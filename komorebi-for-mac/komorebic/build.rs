fn main() {
    if std::fs::metadata("applications.json").is_err() {
        let applications_json = reqwest::blocking::get(
            "https://raw.githubusercontent.com/LGUG2Z/komorebi-application-specific-configuration/master/applications.mac.json"
        ).unwrap().text().unwrap();
        std::fs::write("applications.json", applications_json).unwrap();
    }
}
