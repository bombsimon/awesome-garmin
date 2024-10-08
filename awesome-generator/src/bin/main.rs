use clap::Parser;

#[derive(Parser)]
#[command(name = "garmin")]
#[command(bin_name = "garmin")]
enum AwesomeGarminCli {
    /// Generate the README from `awesome.toml`.
    GenerateReadme,
    /// Compare what's in `awesome.toml` with a search result from Connect IQ apps.
    Compare(SearchArgs),
    /// Search Connect IQ for a keyword and print resources with source code.
    Search(SearchArgs),
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
        AwesomeGarminCli::GenerateReadme => awesome_generator::generate_readme().await.unwrap(),
        AwesomeGarminCli::Compare(args) => awesome_generator::compare(&args.keyword).await.unwrap(),
        AwesomeGarminCli::Search(args) => {
            awesome_generator::search::print_resource_urls(&args.keyword)
                .await
                .unwrap()
        }
    }
}
