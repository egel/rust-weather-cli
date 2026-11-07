mod config;
mod geo;
mod tui;
mod weather;

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let mut terminal = ratatui::init();
    let result = tui::run(&mut terminal).await;
    ratatui::restore();
    result
}
