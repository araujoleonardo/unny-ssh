use crate::ipc::{AppError, OperationState, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, VecDeque},
    time::{Duration, Instant},
};
use tauri::ipc::Channel;
use uuid::Uuid;

pub const MAX_PENDING: usize = 32;
pub const MAX_STREAMS: usize = 16;
pub const MAX_MESSAGE: usize = 65536;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamEvent {
    pub operation_id: Uuid,
    pub generation: u64,
    pub sequence: u64,
    pub state: OperationState,
    pub data: String,
}
struct Stream {
    generation: u64,
    sequence: u64,
    inflight: Option<(u64, OperationState)>,
    final_queued: bool,
    last_progress: Option<Instant>,
    queue: VecDeque<(OperationState, String)>,
    channel: Channel<StreamEvent>,
}
#[derive(Default)]
pub struct Streams {
    entries: HashMap<Uuid, Stream>,
}
impl Streams {
    pub fn open(&mut self, generation: u64, channel: Channel<StreamEvent>) -> Result<Uuid> {
        if self.entries.len() >= MAX_STREAMS {
            return Err(AppError::new("BUSY", "Limite de streams atingido."));
        }
        let id = Uuid::new_v4();
        self.entries.insert(
            id,
            Stream {
                generation,
                sequence: 0,
                inflight: None,
                final_queued: false,
                last_progress: None,
                queue: VecDeque::new(),
                channel,
            },
        );
        Ok(id)
    }
    /// Aggregate progress snapshots to at most 10 updates/s. Producers supply
    /// cumulative counters, so skipped intermediate snapshots lose no byte count.
    /// Completion/cancellation/failure bypasses throttling and is always final.
    pub fn progress(
        &mut self,
        id: Uuid,
        state: OperationState,
        processed: u64,
        total: Option<u64>,
        now: Instant,
    ) -> Result<()> {
        if total.is_some_and(|n| processed > n) {
            return Err(AppError::invalid());
        }
        let stream = self.entries.get_mut(&id).ok_or_else(unknown)?;
        if stream.final_queued {
            return Err(AppError::invalid());
        }
        if state == OperationState::Running
            && stream.last_progress.is_some_and(|last| {
                now.saturating_duration_since(last) < Duration::from_millis(100)
            })
        {
            return Ok(());
        }
        let data = serde_json::json!({"processed":processed,"total":total}).to_string();
        self.enqueue(id, state, data)?;
        if let Some(stream) = self.entries.get_mut(&id) {
            stream.last_progress = Some(now);
        }
        Ok(())
    }
    pub fn enqueue(&mut self, id: Uuid, state: OperationState, data: String) -> Result<()> {
        let s = self.entries.get_mut(&id).ok_or_else(unknown)?;
        if s.final_queued {
            return Err(AppError::invalid());
        }
        if s.queue.len() >= MAX_PENDING || data.len() > MAX_MESSAGE {
            return Err(AppError::new(
                "BACKPRESSURE",
                "Aguarde o consumidor antes de produzir mais dados.",
            ));
        }
        s.final_queued = state != OperationState::Running;
        s.queue.push_back((state, data));
        self.pump(id)
    }
    fn pump(&mut self, id: Uuid) -> Result<()> {
        let s = self.entries.get_mut(&id).ok_or_else(unknown)?;
        if s.inflight.is_some() {
            return Ok(());
        }
        if let Some((state, data)) = s.queue.pop_front() {
            s.sequence += 1;
            let event = StreamEvent {
                operation_id: id,
                generation: s.generation,
                sequence: s.sequence,
                state,
                data,
            };
            if s.channel.send(event).is_err() {
                self.entries.remove(&id);
                return Err(AppError::new("INTERRUPTED", "O consumidor foi encerrado."));
            }
            s.inflight = Some((s.sequence, state));
        }
        Ok(())
    }
    pub fn ack(&mut self, id: Uuid, generation: u64, sequence: u64) -> Result<()> {
        let s = self.entries.get_mut(&id).ok_or_else(unknown)?;
        if s.generation != generation {
            return Err(AppError::new("STALE_GENERATION", "Stream expirado."));
        }
        if s.inflight.map(|(n, _)| n) != Some(sequence) {
            return Err(AppError::invalid());
        }
        let terminal = s
            .inflight
            .map(|(_, state)| state != OperationState::Running)
            .unwrap_or(false);
        s.inflight = None;
        if terminal {
            self.entries.remove(&id);
            return Ok(());
        }
        self.pump(id)
    }
    pub fn cancel(&mut self, id: Uuid, generation: u64) -> Result<()> {
        let s = self.entries.get(&id).ok_or_else(unknown)?;
        if s.generation != generation {
            return Err(AppError::new("STALE_GENERATION", "Stream expirado."));
        }
        // Discard queued content; the cancellation response is the final receipt.
        self.entries.remove(&id);
        Ok(())
    }
    pub fn invalidate(&mut self) {
        self.entries.clear();
    }
}
fn unknown() -> AppError {
    AppError::new("UNKNOWN_RESOURCE", "Recurso desconhecido ou encerrado.")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn progress_aggregates_at_ten_hz_with_final_snapshot() {
        let mut streams = Streams::default();
        let id = streams.open(1, Channel::new(|_| Ok(()))).unwrap();
        let now = Instant::now();
        streams
            .progress(id, OperationState::Running, 1, Some(100), now)
            .unwrap();
        streams.ack(id, 1, 1).unwrap();
        streams
            .progress(
                id,
                OperationState::Running,
                20,
                Some(100),
                now + Duration::from_millis(30),
            )
            .unwrap();
        assert_eq!(streams.entries[&id].sequence, 1);
        streams
            .progress(
                id,
                OperationState::Running,
                30,
                Some(100),
                now + Duration::from_millis(100),
            )
            .unwrap();
        streams.ack(id, 1, 2).unwrap();
        streams
            .progress(
                id,
                OperationState::Completed,
                100,
                Some(100),
                now + Duration::from_millis(105),
            )
            .unwrap();
        assert_eq!(streams.entries[&id].sequence, 3);
        streams.ack(id, 1, 3).unwrap();
        assert!(streams.entries.is_empty());
    }
    #[test]
    fn final_ack_releases_stream_slot() {
        let mut streams = Streams::default();
        let id = streams.open(1, Channel::new(|_| Ok(()))).unwrap();
        streams
            .enqueue(id, OperationState::Completed, String::new())
            .unwrap();
        assert!(
            streams
                .enqueue(id, OperationState::Running, String::new())
                .is_err()
        );
        streams.ack(id, 1, 1).unwrap();
        assert!(streams.entries.is_empty());
    }
    #[test]
    fn slow_consumer_bounded_and_cancelled() {
        let mut streams = Streams::default();
        let channel = Channel::new(|_| Ok(()));
        let id = streams.open(1, channel).unwrap();
        for _ in 0..=MAX_PENDING {
            streams
                .enqueue(id, OperationState::Running, "x".into())
                .unwrap();
        }
        assert_eq!(
            streams
                .enqueue(id, OperationState::Running, "x".into())
                .unwrap_err()
                .code,
            "BACKPRESSURE"
        );
        assert!(streams.ack(id, 2, 1).is_err());
        assert!(streams.ack(id, 1, 9).is_err());
        streams.ack(id, 1, 1).unwrap();
        streams.cancel(id, 1).unwrap();
        assert!(streams.ack(id, 1, 2).is_err());
        let id = streams.open(1, Channel::new(|_| Ok(()))).unwrap();
        streams.invalidate();
        assert!(streams.ack(id, 1, 1).is_err());
    }
}
