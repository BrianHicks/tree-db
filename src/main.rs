use clap::Parser;
use tracing_error::ErrorLayer;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

mod export;
mod loader;

fn main() {
    let subscriber = tracing_subscriber::Registry::default()
        .with(ErrorLayer::default())
        .with(
            tracing_subscriber::fmt::layer()
                .with_span_events(FmtSpan::NEW)
                .with_writer(std::io::stderr),
        )
        .with(
            EnvFilter::try_from_default_env()
                // TODO: default to `info` eventually
                .or_else(|_| EnvFilter::try_new("trace"))
                .unwrap(),
        );

    tracing::subscriber::set_global_default(subscriber)
        .expect("could not initialize tracing subscribers");

    color_eyre::install().expect("could not initialize error handling");

    let opts = export::ExporterConfig::parse();

    if let Err(err) = opts.run() {
        eprintln!("{err:?}");
        std::process::exit(1);
    }
}
