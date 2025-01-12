use komorebi::StaticConfig;

#[test]
fn backwards_compat() {
    let root = vec!["0.1.17", "0.1.18", "0.1.19"];
    let docs = vec![
        "0.1.20", "0.1.21", "0.1.22", "0.1.23", "0.1.24", "0.1.25", "0.1.26", "0.1.27", "0.1.28",
        "0.1.29", "0.1.30", "0.1.31", "0.1.32", "0.1.33",
    ];

    let mut versions = vec![];

    let client = reqwest::blocking::Client::new();

    for version in root {
        let request = client.get(format!("https://raw.githubusercontent.com/LGUG2Z/komorebi/refs/tags/v{version}/komorebi.example.json")).header("User-Agent", "komorebi-backwards-compat-test").build().unwrap();
        versions.push((version, client.execute(request).unwrap().text().unwrap()));
    }

    for version in docs {
        let request = client.get(format!("https://raw.githubusercontent.com/LGUG2Z/komorebi/refs/tags/v{version}/docs/komorebi.example.json")).header("User-Agent", "komorebi-backwards-compat-test").build().unwrap();
        versions.push((version, client.execute(request).unwrap().text().unwrap()));
    }

    for (version, config) in versions {
        println!("{version}");
        StaticConfig::read_raw(&config).unwrap();
    }
}
