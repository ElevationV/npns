mod fs;
mod ui;
mod app;

use crate::app::App;
use crate::ui::{RawMode, Screen};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let start_dir = std::env::current_dir()?;

    // Enter raw mode (restored automatically on drop)
    let _raw = RawMode::enter()?;

    // Allocate the screen (enters alternate screen, hides cursor)
    let mut scr = Screen::init()?;

    let mut app = App::new(start_dir)?;
    let res = app.run(&mut scr);

    // Leave alternate screen + restore cursor
    scr.shutdown();

    if let Err(err) = res {
        eprintln!("{err:?}");
    }

    Ok(())
}