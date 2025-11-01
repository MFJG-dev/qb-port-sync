use qb_port_sync::JsonReport;

#[test]
fn json_report_renders_single_line() {
    let mut report = JsonReport::new("file");
    report.detected_port = Some(51820);
    report.applied = true;
    report.verified = true;
    report.note = String::from("ttl=300s");
    let line = report.line().expect("serialise json report");
    assert!(line.contains("\"strategy\":\"file\""));
    assert!(line.contains("\"detected_port\":51820"));
    assert!(!line.contains("error"));
}
