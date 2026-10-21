use anyhow::Result;
use indicatif::{ProgressBar, ProgressStyle};
use std::{
    io::{self, IsTerminal as _},
    time::{Duration, Instant},
};

/// Show a blocking stage on stderr without changing the command's stdout report.
pub fn run<T>(label: &'static str, work: impl FnOnce() -> Result<T>) -> Result<T> {
    let stage = Stage::start(label)?;
    let value = work()?;
    stage.finish(label);
    Ok(value)
}

struct Stage {
    spinner: Option<ProgressBar>,
    started: Instant,
}

impl Stage {
    fn start(label: &'static str) -> Result<Self> {
        let spinner = if io::stderr().is_terminal() {
            let style =
                ProgressStyle::with_template("{spinner} {msg} [{elapsed_precise} elapsed]")?;
            let spinner = ProgressBar::new_spinner().with_style(style);
            spinner.set_message(label);
            spinner.enable_steady_tick(Duration::from_millis(100));
            Some(spinner)
        } else {
            eprintln!("{label}...");
            None
        };
        Ok(Self {
            spinner,
            started: Instant::now(),
        })
    }

    fn finish(self, label: &str) {
        let elapsed = self.started.elapsed();
        // Clear the animation before writing the completed stage.
        drop(self);
        eprintln!("{label}: done ({:.1}s)", elapsed.as_secs_f64());
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        // Also clear the animation when a stage returns an error.
        if let Some(spinner) = &self.spinner {
            spinner.finish_and_clear();
        }
    }
}
