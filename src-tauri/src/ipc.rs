use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    pub code: &'static str,
    pub message: &'static str,
    pub retryable: bool,
    pub correlation_id: Uuid,
}
impl AppError {
    pub fn new(code: &'static str, message: &'static str) -> Self {
        Self {
            code,
            message,
            retryable: matches!(code, "IO_ERROR" | "BUSY"),
            correlation_id: Uuid::new_v4(),
        }
    }
    pub fn invalid() -> Self {
        Self::new("INVALID_INPUT", "Dados inválidos. Confira os campos.")
    }
    pub fn io() -> Self {
        Self::new(
            "IO_ERROR",
            "Não foi possível gravar ou ler o cofre. Verifique o disco e as permissões.",
        )
    }
}
pub type Result<T> = std::result::Result<T, AppError>;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum OperationState {
    Running,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourceContext {
    pub session_id: Uuid,
    pub generation: u64,
}
