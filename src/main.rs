#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

fn main() {
    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();

    let args = std::env::args_os().collect::<Vec<_>>();
    if anneal_cli::app::should_handle_args(&args) {
        if let Err(error) = anneal_cli::app::run_args(args) {
            for cause in error.chain() {
                if let Some(io_err) = cause.downcast_ref::<std::io::Error>()
                    && io_err.kind() == std::io::ErrorKind::BrokenPipe
                {
                    std::process::exit(0);
                }
            }
            eprintln!("error: {error:#}");
            std::process::exit(1);
        }
    } else {
        anneal_legacy::main_entry();
    }
}
