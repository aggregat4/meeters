use std::time::Duration;

use base64::prelude::*;
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;
use roxmltree::{Document, Node};
use ureq::Agent;

use crate::domain::{Event, EventMetadata, Participant, ResponseStatus};
use crate::meeters_ical::{convert_to_zoommtg, parse_zoom_url};

#[derive(Debug)]
pub enum EwsError {
    AuthFailed,
    Other(String),
}

#[derive(Debug, Clone)]
pub struct EwsConfig {
    pub url: String,
    pub user: String,
}

#[derive(Debug)]
struct CalendarItemSummary {
    id: String,
    change_key: Option<String>,
    summary: String,
    location: String,
    start_timestamp: DateTime<Tz>,
    end_timestamp: DateTime<Tz>,
    all_day: bool,
}

#[derive(Debug, Clone)]
struct EventDetails {
    description: String,
    metadata: EventMetadata,
}

impl EventDetails {
    fn empty() -> Self {
        EventDetails {
            description: String::new(),
            metadata: EventMetadata::empty(),
        }
    }
}

pub fn fetch_events(
    agent: &Agent,
    config: &EwsConfig,
    password: &str,
    local_tz: &Tz,
    start_time: DateTime<Tz>,
    end_time: DateTime<Tz>,
    use_zoommtg: bool,
) -> Result<Vec<Event>, EwsError> {
    let find_response = post_soap(
        agent,
        config,
        password,
        &find_item_request(start_time, end_time),
        "http://schemas.microsoft.com/exchange/services/2006/messages/FindItem",
    )?;
    let summaries = parse_find_item_response(&find_response, local_tz)?;
    if summaries.is_empty() {
        return Ok(Vec::new());
    }

    let body_response = post_soap(
        agent,
        config,
        password,
        &get_item_request(&summaries),
        "http://schemas.microsoft.com/exchange/services/2006/messages/GetItem",
    )?;
    let details = parse_get_item_details(&body_response)?;

    summaries
        .into_iter()
        .map(|item| {
            let details = details
                .iter()
                .find(|(id, _)| id == &item.id)
                .map(|(_, details)| details.clone())
                .unwrap_or_else(EventDetails::empty);
            let description = details.description;
            let mut meeturl = parse_zoom_url(&item.location)
                .or_else(|| parse_zoom_url(&item.summary))
                .or_else(|| parse_zoom_url(&description));

            if use_zoommtg {
                if let Some(url) = meeturl {
                    meeturl = Some(convert_to_zoommtg(&url).unwrap_or(url));
                }
            }

            Ok(Event {
                summary: item.summary,
                description,
                location: item.location,
                meeturl,
                metadata: details.metadata,
                all_day: item.all_day,
                start_timestamp: item.start_timestamp,
                end_timestamp: item.end_timestamp,
            })
        })
        .collect()
}

pub(crate) fn http_agent(timeout: Duration) -> Agent {
    Agent::config_builder()
        .timeout_global(Some(timeout))
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .into()
}

fn post_soap(
    agent: &Agent,
    config: &EwsConfig,
    password: &str,
    soap: &str,
    soap_action: &str,
) -> Result<String, EwsError> {
    let authorization = format!(
        "Basic {}",
        BASE64_STANDARD.encode(format!("{}:{}", config.user, password))
    );
    let mut response = agent
        .post(&config.url)
        .header("Authorization", &authorization)
        .header("Content-Type", "text/xml; charset=utf-8")
        .header("SOAPAction", format!("\"{}\"", soap_action))
        .send(soap)
        .map_err(|e| EwsError::Other(format!("Error calling EWS endpoint: {}", e)))?;
    let status = response.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(EwsError::AuthFailed);
    }
    if !status.is_success() {
        return Err(EwsError::Other(format!(
            "EWS returned HTTP status {}",
            status.as_u16()
        )));
    }
    // Match Reqwest's previous unlimited, lossy UTF-8 decoding. Ureq's
    // convenience string reader otherwise introduces a 10 MiB response limit.
    let bytes = response
        .body_mut()
        .with_config()
        .read_to_vec()
        .map_err(|e| EwsError::Other(format!("Could not read EWS response body: {}", e)))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn find_item_request(start_time: DateTime<Tz>, end_time: DateTime<Tz>) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<soap:Envelope xmlns:soap="http://schemas.xmlsoap.org/soap/envelope/"
  xmlns:t="http://schemas.microsoft.com/exchange/services/2006/types"
  xmlns:m="http://schemas.microsoft.com/exchange/services/2006/messages">
  <soap:Header>
    <t:RequestServerVersion Version="Exchange2016" />
  </soap:Header>
  <soap:Body>
    <m:FindItem Traversal="Shallow">
      <m:ItemShape>
        <t:BaseShape>IdOnly</t:BaseShape>
        <t:AdditionalProperties>
          <t:FieldURI FieldURI="item:Subject" />
          <t:FieldURI FieldURI="calendar:Start" />
          <t:FieldURI FieldURI="calendar:End" />
          <t:FieldURI FieldURI="calendar:Location" />
          <t:FieldURI FieldURI="calendar:IsAllDayEvent" />
        </t:AdditionalProperties>
      </m:ItemShape>
      <m:CalendarView StartDate="{}" EndDate="{}" />
      <m:ParentFolderIds>
        <t:DistinguishedFolderId Id="calendar" />
      </m:ParentFolderIds>
    </m:FindItem>
  </soap:Body>
</soap:Envelope>"#,
        ews_datetime(start_time),
        ews_datetime(end_time)
    )
}

fn get_item_request(items: &[CalendarItemSummary]) -> String {
    let item_ids = items
        .iter()
        .map(|item| match &item.change_key {
            Some(change_key) => format!(
                r#"<t:ItemId Id="{}" ChangeKey="{}" />"#,
                xml_escape_attr(&item.id),
                xml_escape_attr(change_key)
            ),
            None => format!(r#"<t:ItemId Id="{}" />"#, xml_escape_attr(&item.id)),
        })
        .collect::<Vec<_>>()
        .join("");

    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<soap:Envelope xmlns:soap="http://schemas.xmlsoap.org/soap/envelope/"
  xmlns:t="http://schemas.microsoft.com/exchange/services/2006/types"
  xmlns:m="http://schemas.microsoft.com/exchange/services/2006/messages">
  <soap:Header>
    <t:RequestServerVersion Version="Exchange2016" />
  </soap:Header>
  <soap:Body>
    <m:GetItem>
      <m:ItemShape>
        <t:BaseShape>IdOnly</t:BaseShape>
        <t:BodyType>Text</t:BodyType>
        <t:AdditionalProperties>
          <t:FieldURI FieldURI="item:Body" />
          <t:FieldURI FieldURI="calendar:Organizer" />
          <t:FieldURI FieldURI="calendar:RequiredAttendees" />
          <t:FieldURI FieldURI="calendar:OptionalAttendees" />
          <t:FieldURI FieldURI="calendar:Resources" />
        </t:AdditionalProperties>
      </m:ItemShape>
      <m:ItemIds>{}</m:ItemIds>
    </m:GetItem>
  </soap:Body>
</soap:Envelope>"#,
        item_ids
    )
}

fn parse_find_item_response(
    response: &str,
    local_tz: &Tz,
) -> Result<Vec<CalendarItemSummary>, EwsError> {
    let doc = Document::parse(response)
        .map_err(|e| EwsError::Other(format!("Could not parse EWS FindItem response: {}", e)))?;
    soap_fault_or_error(&doc)?;

    doc.descendants()
        .filter(|node| node.has_tag_name("CalendarItem"))
        .map(|node| parse_calendar_item_summary(node, local_tz))
        .collect()
}

fn parse_calendar_item_summary(
    node: Node<'_, '_>,
    local_tz: &Tz,
) -> Result<CalendarItemSummary, EwsError> {
    let item_id = child(node, "ItemId").ok_or_else(|| {
        EwsError::Other("EWS calendar item did not include an ItemId".to_string())
    })?;
    let id = item_id
        .attribute("Id")
        .ok_or_else(|| EwsError::Other("EWS calendar item ItemId had no Id".to_string()))?
        .to_string();
    let change_key = item_id
        .attribute("ChangeKey")
        .map(|value| value.to_string());
    let summary = child_text(node, "Subject").unwrap_or_default();
    let location = child_text(node, "Location").unwrap_or_default();
    let start_timestamp = parse_ews_datetime(
        child_text(node, "Start")
            .as_deref()
            .ok_or_else(|| EwsError::Other(format!("EWS item '{}' had no Start", summary)))?,
        local_tz,
    )?;
    let end_timestamp = parse_ews_datetime(
        child_text(node, "End")
            .as_deref()
            .ok_or_else(|| EwsError::Other(format!("EWS item '{}' had no End", summary)))?,
        local_tz,
    )?;
    let all_day = child_text(node, "IsAllDayEvent")
        .map(|value| value == "true" || value == "1")
        .unwrap_or(false);

    Ok(CalendarItemSummary {
        id,
        change_key,
        summary,
        location,
        start_timestamp,
        end_timestamp,
        all_day,
    })
}

fn parse_get_item_details(response: &str) -> Result<Vec<(String, EventDetails)>, EwsError> {
    let doc = Document::parse(response)
        .map_err(|e| EwsError::Other(format!("Could not parse EWS GetItem response: {}", e)))?;
    soap_fault_or_error(&doc)?;

    doc.descendants()
        .filter(|node| node.has_tag_name("CalendarItem"))
        .map(|node| {
            let item_id = child(node, "ItemId").ok_or_else(|| {
                EwsError::Other("EWS GetItem response item did not include an ItemId".to_string())
            })?;
            let id = item_id
                .attribute("Id")
                .ok_or_else(|| EwsError::Other("EWS GetItem ItemId had no Id".to_string()))?
                .to_string();
            let body = child(node, "Body")
                .map(|body| body_text(body))
                .unwrap_or_default();
            let metadata = EventMetadata {
                organizer: child(node, "Organizer").and_then(mailbox_display_name),
                rooms: attendees(node, "Resources"),
                required_attendees: attendees(node, "RequiredAttendees"),
                optional_attendees: attendees(node, "OptionalAttendees"),
            };
            Ok((
                id,
                EventDetails {
                    description: body,
                    metadata,
                },
            ))
        })
        .collect()
}

fn soap_fault_or_error(doc: &Document<'_>) -> Result<(), EwsError> {
    if let Some(fault) = doc.descendants().find(|node| node.has_tag_name("Fault")) {
        let message = child_text(fault, "faultstring")
            .or_else(|| child_text(fault, "MessageText"))
            .unwrap_or_else(|| "Unknown EWS SOAP fault".to_string());
        return Err(EwsError::Other(format!("EWS SOAP fault: {}", message)));
    }

    if let Some(message) = doc.descendants().find(|node| {
        node.tag_name().name().ends_with("ResponseMessage")
            && node.attribute("ResponseClass") == Some("Error")
    }) {
        let code = child_text(message, "ResponseCode").unwrap_or_else(|| "Unknown".to_string());
        let message_text = child_text(message, "MessageText").unwrap_or_default();
        return Err(EwsError::Other(format!(
            "EWS returned {}: {}",
            code, message_text
        )));
    }

    Ok(())
}

fn body_text(body: Node<'_, '_>) -> String {
    let text = body.text().unwrap_or_default().trim();
    if body.attribute("BodyType") == Some("HTML") {
        html2text::from_read(text.as_bytes(), 120).unwrap_or_else(|_| strip_html_tags(text))
    } else {
        text.to_string()
    }
}

fn attendees(node: Node<'_, '_>, group_name: &str) -> Vec<Participant> {
    child(node, group_name)
        .map(|group| {
            group
                .children()
                .filter(|child| child.has_tag_name("Attendee"))
                .filter_map(participant)
                .collect()
        })
        .unwrap_or_default()
}

fn participant(node: Node<'_, '_>) -> Option<Participant> {
    let name = mailbox_display_name(node)?;
    let response = child_text(node, "ResponseType").map(|value| ResponseStatus::from_ews(&value));
    Some(Participant { name, response })
}

fn mailbox_display_name(node: Node<'_, '_>) -> Option<String> {
    let mailbox = if node.has_tag_name("Mailbox") {
        Some(node)
    } else {
        child(node, "Mailbox")
    }?;

    child_text(mailbox, "Name")
        .or_else(|| child_text(mailbox, "EmailAddress"))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn child<'a>(node: Node<'a, 'a>, name: &str) -> Option<Node<'a, 'a>> {
    node.children().find(|child| child.has_tag_name(name))
}

fn child_text(node: Node<'_, '_>, name: &str) -> Option<String> {
    child(node, name).and_then(|child| child.text().map(|text| text.to_string()))
}

fn parse_ews_datetime(value: &str, local_tz: &Tz) -> Result<DateTime<Tz>, EwsError> {
    if let Ok(datetime) = DateTime::parse_from_rfc3339(value) {
        return Ok(datetime.with_timezone(local_tz));
    }

    let naive = NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S")
        .map_err(|e| EwsError::Other(format!("Could not parse EWS datetime '{}': {}", value, e)))?;
    local_tz
        .from_local_datetime(&naive)
        .single()
        .ok_or_else(|| EwsError::Other(format!("EWS datetime '{}' is ambiguous", value)))
}

fn ews_datetime(value: DateTime<Tz>) -> String {
    value
        .with_timezone(&Utc)
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn xml_escape_attr(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn strip_html_tags(value: &str) -> String {
    let mut result = String::new();
    let mut in_tag = false;
    for character in value.chars() {
        match character {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(character),
            _ => {}
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Timelike;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::thread;

    fn read_request(stream: &mut BufReader<std::net::TcpStream>) -> String {
        let mut headers = String::new();
        loop {
            let mut line = String::new();
            assert!(stream.read_line(&mut line).unwrap() > 0);
            headers.push_str(&line);
            if line == "\r\n" {
                break;
            }
        }
        let length: usize = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse().unwrap())
            })
            .unwrap_or(0);
        let mut body = vec![0; length];
        stream.read_exact(&mut body).unwrap();
        headers + &String::from_utf8(body).unwrap()
    }

    #[test]
    fn soap_reuses_connection_and_sends_current_credentials() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let config = EwsConfig {
            url: format!("http://{}/ews", listener.local_addr().unwrap()),
            user: "user".into(),
        };
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut stream = BufReader::new(stream);
            for password in ["first", "second"] {
                let request = read_request(&mut stream);
                let headers = request.to_ascii_lowercase();
                assert!(request.starts_with("POST /ews HTTP/1.1\r\n"));
                assert!(headers.contains("content-type: text/xml; charset=utf-8\r\n"));
                assert!(headers.contains("soapaction: \"action\"\r\n"));
                assert!(request.contains(&format!(
                    "Basic {}",
                    BASE64_STANDARD.encode(format!("user:{password}"))
                )));
                assert!(request.ends_with("<soap/>"));
                stream
                    .get_mut()
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                    .unwrap();
            }
        });
        let agent = http_agent(Duration::from_secs(2));
        for password in ["first", "second"] {
            assert_eq!(
                post_soap(&agent.clone(), &config, password, "<soap/>", "action").unwrap(),
                "ok"
            );
        }
        server.join().unwrap();
    }

    fn serve_response(
        status: u16,
        body: Vec<u8>,
        delay: Duration,
    ) -> (EwsConfig, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/ews", listener.local_addr().unwrap());
        let redirect = format!("{url}/redirect");
        let handle = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut stream = BufReader::new(stream);
            read_request(&mut stream);
            thread::sleep(delay);
            let headers = format!("HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nLocation: {redirect}\r\nConnection: close\r\n\r\n", body.len());
            let _ = stream.get_mut().write_all(headers.as_bytes());
            let _ = stream.get_mut().write_all(&body);
        });
        (
            EwsConfig {
                url,
                user: "user".into(),
            },
            handle,
        )
    }

    #[test]
    fn soap_honors_no_proxy_environment() {
        const CHILD_URL: &str = "MEETERS_TEST_NO_PROXY_URL";
        if let Ok(url) = std::env::var(CHILD_URL) {
            let agent = http_agent(Duration::from_secs(2));
            let proxy = agent
                .config()
                .proxy()
                .expect("proxy configured by environment");
            assert!(proxy.is_no_proxy(&url.parse().unwrap()));
            let config = EwsConfig {
                url,
                user: "user".into(),
            };
            assert_eq!(
                post_soap(&agent, &config, "password", "body", "action").unwrap(),
                "ok"
            );
            return;
        }

        // Isolate environment changes from other tests and background threads.
        let (config, server) = serve_response(200, b"ok".to_vec(), Duration::ZERO);
        let unused_proxy = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap());
        child.args([
            "--exact",
            "ews::tests::soap_honors_no_proxy_environment",
            "--nocapture",
        ]);
        for key in [
            "ALL_PROXY",
            "all_proxy",
            "HTTP_PROXY",
            "http_proxy",
            "HTTPS_PROXY",
            "https_proxy",
            "NO_PROXY",
            "no_proxy",
        ] {
            child.env_remove(key);
        }
        let output = child
            .env(CHILD_URL, config.url)
            .env(
                "HTTP_PROXY",
                format!("http://{}", unused_proxy.local_addr().unwrap()),
            )
            .env("NO_PROXY", "127.0.0.1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        server.join().unwrap();
    }

    #[test]
    fn soap_preserves_auth_errors_and_rejects_redirects_and_server_errors() {
        for status in [401, 403, 302, 500] {
            let (config, server) = serve_response(status, vec![], Duration::ZERO);
            let error = post_soap(
                &http_agent(Duration::from_secs(2)),
                &config,
                "password",
                "body",
                "action",
            )
            .unwrap_err();
            match error {
                EwsError::AuthFailed => assert!([401, 403].contains(&status)),
                EwsError::Other(message) => {
                    assert_eq!(message, format!("EWS returned HTTP status {status}"))
                }
            }
            server.join().unwrap();
        }
    }

    #[test]
    fn soap_preserves_large_responses_and_lossy_utf8() {
        let mut body = vec![b'a'; 10 * 1024 * 1024 + 1];
        body.push(0xff);
        let expected = String::from_utf8_lossy(&body).into_owned();
        let (config, server) = serve_response(200, body, Duration::ZERO);
        assert_eq!(
            post_soap(
                &http_agent(Duration::from_secs(5)),
                &config,
                "password",
                "body",
                "action"
            )
            .unwrap(),
            expected
        );
        server.join().unwrap();
    }

    #[test]
    fn soap_times_out_waiting_for_response() {
        let (config, server) = serve_response(200, vec![], Duration::from_millis(250));
        let error = post_soap(
            &http_agent(Duration::from_millis(50)),
            &config,
            "password",
            "body",
            "action",
        )
        .unwrap_err();
        assert!(
            matches!(error, EwsError::Other(message) if message.starts_with("Error calling EWS endpoint:"))
        );
        server.join().unwrap();
    }

    #[test]
    fn parses_calendar_item_summary() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/"
  xmlns:m="http://schemas.microsoft.com/exchange/services/2006/messages"
  xmlns:t="http://schemas.microsoft.com/exchange/services/2006/types">
  <s:Body>
    <m:FindItemResponse>
      <m:ResponseMessages>
        <m:FindItemResponseMessage ResponseClass="Success">
          <m:RootFolder>
            <t:Items>
              <t:CalendarItem>
                <t:ItemId Id="abc" ChangeKey="def" />
                <t:Subject>Planning</t:Subject>
                <t:Location>https://example.zoom.us/j/123</t:Location>
                <t:Start>2026-05-19T08:00:00Z</t:Start>
                <t:End>2026-05-19T08:30:00Z</t:End>
                <t:IsAllDayEvent>false</t:IsAllDayEvent>
              </t:CalendarItem>
            </t:Items>
          </m:RootFolder>
        </m:FindItemResponseMessage>
      </m:ResponseMessages>
    </m:FindItemResponse>
  </s:Body>
</s:Envelope>"#;

        let items = parse_find_item_response(xml, &chrono_tz::Europe::Berlin).unwrap();

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "abc");
        assert_eq!(items[0].summary, "Planning");
        assert_eq!(items[0].start_timestamp.hour(), 10);
    }

    #[test]
    fn parses_details_from_get_item_response() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/"
  xmlns:m="http://schemas.microsoft.com/exchange/services/2006/messages"
  xmlns:t="http://schemas.microsoft.com/exchange/services/2006/types">
  <s:Body>
    <m:GetItemResponse>
      <m:ResponseMessages>
        <m:GetItemResponseMessage ResponseClass="Success">
          <m:Items>
            <t:CalendarItem>
              <t:ItemId Id="abc" />
              <t:Body BodyType="Text">Join at https://example.zoom.us/j/123</t:Body>
              <t:Organizer>
                <t:Mailbox>
                  <t:Name>Alice Example</t:Name>
                  <t:EmailAddress>alice@example.com</t:EmailAddress>
                </t:Mailbox>
              </t:Organizer>
              <t:RequiredAttendees>
                <t:Attendee>
                  <t:Mailbox>
                    <t:Name>Bob Example</t:Name>
                  </t:Mailbox>
                </t:Attendee>
              </t:RequiredAttendees>
              <t:OptionalAttendees>
                <t:Attendee>
                  <t:Mailbox>
                    <t:EmailAddress>carol@example.com</t:EmailAddress>
                  </t:Mailbox>
                </t:Attendee>
              </t:OptionalAttendees>
              <t:Resources>
                <t:Attendee>
                  <t:Mailbox>
                    <t:Name>Room 3A</t:Name>
                  </t:Mailbox>
                  <t:ResponseType>Decline</t:ResponseType>
                </t:Attendee>
              </t:Resources>
            </t:CalendarItem>
          </m:Items>
        </m:GetItemResponseMessage>
      </m:ResponseMessages>
    </m:GetItemResponse>
  </s:Body>
</s:Envelope>"#;

        let details = parse_get_item_details(xml).unwrap();

        assert_eq!(details.len(), 1);
        assert_eq!(details[0].0, "abc");
        assert_eq!(
            details[0].1.description,
            "Join at https://example.zoom.us/j/123"
        );
        assert_eq!(
            details[0].1.metadata.organizer.as_deref(),
            Some("Alice Example")
        );
        assert_eq!(
            details[0].1.metadata.required_attendees[0].name,
            "Bob Example"
        );
        assert_eq!(
            details[0].1.metadata.optional_attendees[0].name,
            "carol@example.com"
        );
        assert_eq!(details[0].1.metadata.rooms[0].name, "Room 3A");
        assert_eq!(
            details[0].1.metadata.rooms[0].response,
            Some(ResponseStatus::Declined)
        );
    }
}
