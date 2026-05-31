mod fs;
mod ui;
mod app;

use crate::app::App;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let start_dir = std::env::current_dir()?;
    let mut tui = crate::ui::tui::init()?;
    let mut app = App::new(start_dir)?;
    let res = app.run(&mut tui);
    crate::ui::tui::shutdown()?;
    if let Err(err) = res {
        eprintln!("{err:?}");
    }
    Ok(())
}