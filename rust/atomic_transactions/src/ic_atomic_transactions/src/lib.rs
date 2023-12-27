use candid::{CandidType, Deserialize};

#[derive(Default, Copy, Clone, Debug, CandidType, Deserialize)]
pub struct Configuration {
    pub infinite_prepare: bool,
    pub stop_on_prepare: bool,
}
