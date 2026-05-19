use std::time::Duration;

use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;
use reqwest::blocking::{Client, Response};
use reqwest::header::CONTENT_TYPE;
use reqwest::redirect::Policy;
use roxmltree::{Document, Node};

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
    config: &EwsConfig,
    password: &str,
    local_tz: &Tz,
    start_time: DateTime<Tz>,
    end_time: DateTime<Tz>,
    use_zoommtg: bool,
) -> Result<Vec<Event>, EwsError> {
    let find_response = post_soap(
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

fn post_soap(
    config: &EwsConfig,
    password: &str,
    soap: &str,
    soap_action: &str,
) -> Result<String, EwsError> {
    let client = Client::builder()
        .http1_only()
        .redirect(Policy::none())
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| EwsError::Other(format!("Could not build EWS HTTP client: {}", e)))?;
    let response = send_ews_post(
        &client,
        &config.url,
        &config.user,
        password,
        soap,
        soap_action,
    )?;
    let status = response.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(EwsError::AuthFailed);
    }
    response_to_text(response)
}

fn send_ews_post(
    client: &Client,
    url: &str,
    user: &str,
    password: &str,
    soap: &str,
    soap_action: &str,
) -> Result<Response, EwsError> {
    client
        .post(url)
        .basic_auth(user, Some(password))
        .header(CONTENT_TYPE, "text/xml; charset=utf-8")
        .header("SOAPAction", format!("\"{}\"", soap_action))
        .body(soap.to_string())
        .send()
        .map_err(|e| EwsError::Other(format!("Error calling EWS endpoint: {}", e)))
}

fn response_to_text(response: Response) -> Result<String, EwsError> {
    let status = response.status();
    if !status.is_success() {
        return Err(EwsError::Other(format!(
            "EWS returned HTTP status {}",
            status.as_u16()
        )));
    }
    response
        .text()
        .map_err(|e| EwsError::Other(format!("Could not read EWS response body: {}", e)))
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
