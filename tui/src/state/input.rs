#[derive(Debug, Default, Clone)]
pub struct InputState {
    pub text: String,
    pub cursor: usize,
    pub history: Vec<String>,
    pub history_index: Option<usize>,
}

#[derive(Debug)]
pub struct TabCompletion {
    pub prefix: String,
    pub candidates: Vec<String>,
    pub index: usize,
    pub insert_start: usize,
}
