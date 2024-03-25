fn main() {
    if std::fs::metadata("applications.yaml").is_err() {
        let applications_yaml = reqwest::blocking::get(
        "https://raw.githubusercontent.com/LGUG2Z/komorebi-application-specific-configuration/master/applications.yaml"
    ).unwrap().text().unwrap();
        std::fs::write("applications.yaml", applications_yaml).unwrap();
    }
}
