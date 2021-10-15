use ical::parser::ical::component::IcalEvent;
use ical::property::Property;

pub fn find_property_value(properties: &[Property], name: &str) -> Option<String> {
    for property in properties {
        if property.name == name {
            // obviously this clone works but I don't like it, as_ref() didn't seem to do it
            // still do not understand the semantics I should be using here
            return property.value.clone();
        }
    }
    None
}

pub fn find_property<'a>(properties: &'a [Property], name: &str) -> Option<&'a Property> {
    for property in properties {
        if property.name == name {
            return Some(property);
        }
    }
    None
}

pub fn find_param<'a>(params: &'a [(String, Vec<String>)], name: &str) -> Option<&'a [String]> {
    for param in params {
        let (param_name, values) = param;
        if param_name == name {
            return Some(values);
        }
    }
    None
}

pub fn format_param_values(param_values: &[String]) -> String {
    param_values
        .iter()
        .map(|param_val| {
            if param_val.contains(' ') {
                format!("\"{}\"", param_val)
            } else {
                param_val.to_string()
            }
        })
        .collect::<Vec<String>>()
        .join(",")
}

pub fn params_to_string(params: &[(String, Vec<String>)]) -> String {
    if params.is_empty() {
        "".to_string()
    } else {
        return format!(
            ";{}",
            params
                .iter()
                .map(|param| format!("{}={}", param.0, format_param_values(&param.1)))
                .collect::<Vec<String>>()
                .join(",")
        );
    }
}

pub fn prop_to_string(prop: &Property) -> String {
    return format!(
        "{}{}:{}",
        prop.name,
        params_to_string(prop.params.as_ref().unwrap_or(&vec![])),
        prop.value.as_ref().unwrap_or(&"".to_string())
    );
}

pub fn properties_to_string(properties: &[Property]) -> String {
    properties
        .iter() // "interesting" note here: i was getting an E0507 when using into_iter since that apparenty takes ownership. and iter is just return refs
        .map(|p| prop_to_string(p))
        .collect::<Vec<String>>()
        .join("\n")
}

pub fn is_ical_date(prop: &Property) -> bool {
    prop.params.is_some()
        && find_param(prop.params.as_ref().unwrap(), "VALUE").is_some()
        && &find_param(prop.params.as_ref().unwrap(), "VALUE").unwrap()[0] == "DATE"
}

#[allow(dead_code)]
pub fn ical_event_to_string(event: &IcalEvent) -> String {
    properties_to_string(&event.properties)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ical::parser::Component;

    #[test]
    fn ical_to_string_empty_ical_event() {
        assert_eq!("", ical_event_to_string(&IcalEvent::new()));
    }

    #[test]
    fn ical_to_string_one_prop_with_value() {
        let mut event = IcalEvent::new();
        let mut prop = Property::new();
        prop.name = "DESCRIPTION".to_string();
        prop.value = Some("foobar".to_string());
        event.add_property(prop);
        assert_eq!("DESCRIPTION:foobar", ical_event_to_string(&event));
    }

    #[test]
    fn ical_to_string_one_prop_with_no_value() {
        let mut event = IcalEvent::new();
        let mut prop = Property::new();
        prop.name = "DESCRIPTION".to_string();
        event.add_property(prop);
        assert_eq!("DESCRIPTION:", ical_event_to_string(&event));
    }

    #[test]
    fn ical_to_string_two_props() {
        let mut event = IcalEvent::new();
        let mut prop = Property::new();
        prop.name = "FOO".to_string();
        prop.value = Some("bar".to_string());
        event.add_property(prop);

        prop = Property::new();
        prop.name = "baz".to_string();
        prop.value = Some("qux".to_string());
        event.add_property(prop);

        assert_eq!("FOO:bar\nbaz:qux", ical_event_to_string(&event));
    }
}
