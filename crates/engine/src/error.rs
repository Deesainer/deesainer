use deesainer_doc::DocError;
use deesainer_ops::OpError;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum EngineError {
    #[error("unknown command {0:?}; call command_list for the available ids")]
    UnknownCommand(String),
    #[error("command {0:?} is not enabled in the current state")]
    Disabled(String),
    #[error("command {command:?}: params must be a JSON object")]
    ParamsNotObject { command: String },
    #[error("command {command:?}: unknown param {key:?}; accepted: {accepted}")]
    UnknownParam {
        command: String,
        key: String,
        accepted: String,
    },
    #[error("command {command:?}: missing required param {key:?}")]
    MissingParam { command: String, key: String },
    #[error("command {command:?}: param {key:?} {reason}")]
    BadParam {
        command: String,
        key: String,
        reason: String,
    },
    #[error("command {command:?}: {reason}")]
    Invalid { command: String, reason: String },
    #[error(transparent)]
    Op(#[from] OpError),
    #[error(transparent)]
    Doc(#[from] DocError),
    #[error("duplicate command id {0:?} in the registry")]
    DuplicateCommand(String),
    #[error("internal error: {0}")]
    Internal(String),
}
