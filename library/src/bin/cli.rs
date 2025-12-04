use library::run;
use library::LibraryError;

fn main() -> Result<(), LibraryError> {
    run(std::env::args().collect())
}
