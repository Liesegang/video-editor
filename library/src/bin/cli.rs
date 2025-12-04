use std::error::Error;
use library::run;

fn main() -> Result<(), Box<dyn Error>> {
    run(std::env::args().collect())
}
