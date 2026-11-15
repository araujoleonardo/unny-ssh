use crate::ipc::{AppError, Result};
use std::time::{Duration, Instant};
use uuid::Uuid;
#[derive(Default)]
pub struct ResetConfirmation {
    pending: Option<(Uuid, u64, Instant)>,
}
impl ResetConfirmation {
    pub fn issue(&mut self, generation: u64) -> Uuid {
        let id = Uuid::new_v4();
        self.pending = Some((id, generation, Instant::now()));
        id
    }
    pub fn consume(&mut self, id: Uuid, generation: u64) -> Result<()> {
        if let Some((expected, owner, created)) = self.pending.take() {
            if expected == id && owner == generation && created.elapsed() < Duration::from_secs(300)
            {
                return Ok(());
            }
        }
        Err(AppError::new(
            "CONFIRMATION_EXPIRED",
            "A confirmação expirou. Confira o aviso e tente novamente.",
        ))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn confirmation_bound_to_generation_and_single_use() {
        let mut c = ResetConfirmation::default();
        let id = c.issue(1);
        assert!(c.consume(id, 2).is_err());
        let id = c.issue(2);
        assert!(c.consume(id, 2).is_ok());
        assert!(c.consume(id, 2).is_err());
        let id = Uuid::new_v4();
        c.pending = Some((id, 2, Instant::now() - Duration::from_secs(301)));
        assert!(c.consume(id, 2).is_err());
    }
}
