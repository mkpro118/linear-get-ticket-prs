use std::path::Path;

fn main() {
    let output_dir = Path::new("docs");
    linear_get_ticket_prs::cli::generate_docs(output_dir)
        .expect("failed to generate docs");
    eprintln!("docs generated in {}", output_dir.display());
}
