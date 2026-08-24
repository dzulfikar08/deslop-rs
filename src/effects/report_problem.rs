//! Ports `Effects/ReportProblem.hs` — collects problems across worker tasks.

use std::sync::Mutex;

use crate::problem::Problem;

#[derive(Default)]
pub struct ProblemSink(Mutex<Vec<Problem>>);

impl ProblemSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn report(&self, p: Problem) {
        self.0.lock().expect("problem sink poisoned").push(p);
    }

    pub fn take(&self) -> Vec<Problem> {
        let mut guard = self.0.lock().expect("problem sink poisoned");
        std::mem::take(&mut *guard)
    }
}
