use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: aien-campaign-verifier <bundle_dir> <spec_dir>");
        return ExitCode::from(2);
    }

    let report = aien_campaign_verifier::verify_bundle(Path::new(&args[1]), Path::new(&args[2]));
    print!("{}", report.render());
    if report.passed() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
