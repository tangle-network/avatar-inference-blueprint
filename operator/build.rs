use std::path::PathBuf;

fn main() {
    println!("cargo::rerun-if-changed=src");

    let blueprint_metadata = serde_json::json!({
        "name": "avatar-inference",
        "description": "AI avatar generation and talking-head synthesis operator via Tangle",
        "version": env!("CARGO_PKG_VERSION"),
        "manager": {
            "Evm": "AvatarBSM"
        },
        "master_revision": "Latest",
        "jobs": [
            {
                "name": "generate_avatar",
                "job_index": 0,
                "description": "Generate AI avatar from text/image input",
                "inputs": ["(string,string,string,uint32,uint32)"],
                "outputs": ["(bytes,string,uint32,uint32)"],
                "required_results": 1,
                "execution": "local"
            }
        ]
    });

    let json = serde_json::to_string_pretty(&blueprint_metadata).unwrap();
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let workspace_root = manifest_dir.parent().expect("workspace root");
    std::fs::write(workspace_root.join("blueprint.json"), json.as_bytes()).unwrap();
}
