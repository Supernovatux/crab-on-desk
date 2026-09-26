use crab_widget::window::WindowError;
use snafu::Snafu;

#[derive(Debug, Snafu)]
enum WidgetError {
    #[snafu(display("window error"))]
    #[snafu(context(false))]
    Window { source: WindowError },
}

#[snafu::report]
fn main() -> Result<(), WidgetError> {
    println!("Hello, world!");
    let window = crab_widget::window::create()?;
    window.run()?;
    Ok(())
}
