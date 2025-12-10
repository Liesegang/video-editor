use library::LibraryError;
use library::run;

fn main() -> Result<(), LibraryError> {
    run(std::env::args().collect())
}
