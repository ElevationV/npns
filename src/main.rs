mod fs;
mod ui;
mod app;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let start_dir = std::env::current_dir()?;
    let mut tui = crate::ui::tui::init()?;
    let mut app = App::new(start_dir.clone())?;
    let res = app.run(&mut tui);
    crate::ui::tui::shutdown()?;
    use crate::app::{App, ExitAction};
    
    match res {
        Ok(ExitAction::ChangeTo(path)) => {
            eprintln!("NPNS_PATH:{}", path);
        }
        Ok(ExitAction::Stay) => {
        }
        Err(err) => {
            eprintln!("Error: {}", err);
            std::process::exit(1);
        }
    }
    Ok(())
}