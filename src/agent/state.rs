use serde::{Deserialize, Serialize};

/// Máquina de estados do agente Plan-and-Solve.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AgentState {
    Planning,
    AwaitingApproval {
        plan: Vec<String>,
    },
    Executing {
        plan: Vec<String>,
        current_step: usize,
    },
    Synthesizing {
        step_results: Vec<String>,
    },
    Done {
        result: String,
    },
    Cancelled {
        feedback: String,
    },
}

impl AgentState {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Planning => "Planning",
            Self::AwaitingApproval { .. } => "AwaitingApproval",
            Self::Executing { .. } => "Executing",
            Self::Synthesizing { .. } => "Synthesizing",
            Self::Done { .. } => "Done",
            Self::Cancelled { .. } => "Cancelled",
        }
    }
}
