use clap::Parser;

#[derive(Parser, Debug)]
#[command(about = "Compare heap, pool, and arena allocation in the 2D particle engine")]
struct Args {
    #[arg(long, default_value = "all")]
    allocator: String,

    #[arg(long, default_value = "10000,100000,1000000,5000000", value_delimiter = ',')]
    counts: Vec<usize>,

    #[arg(long, default_value_t = 2)]
    warmups: u32,

    #[arg(long, default_value_t = 10)]
    trials: u32,

    #[arg(long, default_value_t = 100)]
    updates: u32,

    #[arg(long, default_value = "results/results.csv")]
    output: String,
}

fn main() {
    let args = Args::parse();
    println!("{args:#?}");
    todo!();
}
