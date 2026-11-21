use clap::Parser;

#[derive(Parser)]
#[command(name = "garmin")]
#[command(bin_name = "garmin")]
enum AwesomeGarminCli {
    /// Generate the README from `awesome.toml`.
    GenerateReadme(GenerateReadmeArgs),
    /// Compare what's in `awesome.toml` with a search result from Connect IQ apps.
    Compare(SearchArgs),
    /// Search Connect IQ for a keyword and print resources with source code.
    Search(SearchArgs),
}

#[derive(clap::Args)]
struct GenerateReadmeArgs {
    /// Only include up to this many resources per section, spread across different hosts such as
    /// GitHub and GitLab. Useful for quick test runs without hitting every API.
    #[arg(long)]
    sample: Option<usize>,
}

#[derive(clap::Args)]
#[command(about, long_about = "Search for keywords")]
struct SearchArgs {
    /// Keyword to search for, e.g. `tennis`.
    keyword: String,
}

#[tokio::main]
async fn main() {
    match AwesomeGarminCli::parse() {
        AwesomeGarminCli::GenerateReadme(args) => awesome_generator::generate_readme(args.sample)
            .await
            .unwrap(),
        AwesomeGarminCli::Compare(args) => awesome_generator::compare(&args.keyword).await.unwrap(),
        AwesomeGarminCli::Search(args) => {
            awesome_generator::search::print_resource_urls(&args.keyword)
                .await
                .unwrap()
        }
    }
}
