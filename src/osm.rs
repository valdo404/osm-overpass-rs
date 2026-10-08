use std::collections::HashMap;
use serde::{Deserialize, Deserializer, Serialize, de::Visitor};

/// The basic component of OpenStreetMap's data model. Comes in three variants: [Node], [Way], and [Relation].
///
/// [wiki](https://wiki.openstreetmap.org/wiki/Elements)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all="lowercase", tag="type")]
pub enum Element {
    Node(Node),
    Way(Way),
    Relation(Relation),
}

/// The identifier of an [Element], independent of the specific variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", tag = "type", content = "ref")]
pub enum ElementId {
    Node(i64),
    Way(i64),
    Relation(i64),
}

/// A position on the map, in degrees.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub lat: f64,
    pub lon: f64,
}

impl Element {
    /// The [ElementId] of this element.
    pub fn id(&self) -> ElementId {
        match self {
            Self::Node(n) => ElementId::Node(n.id),
            Self::Way(w) => ElementId::Way(w.id),
            Self::Relation(r) => ElementId::Relation(r.id),
        }
    }

    /// The value of this element's tag with the given name, if one exists.
    pub fn tag(&self, name: &str) -> Option<&str> {
        let tags = match self {
            Self::Node(n) => &n.tags,
            Self::Way(w) => &w.tags,
            Self::Relation(r) => &r.tags,
        };
        tags.get(name).map(|s| s.as_str())
    }

    /// An iterator of tag values on this element, composed of key/value tuples.
    pub fn tags(&self) -> impl ExactSizeIterator<Item=(&str, &str)> {
        let tags = match self {
            Self::Node(n) => &n.tags,
            Self::Way(w) => &w.tags,
            Self::Relation(r) => &r.tags,
        };
        tags.iter().map(|(k,v)| (k.as_str(), v.as_str()))
    }

    /// The center of this element's bounding box, when the query asked for it (`out center`).
    /// Nodes have none: see [Element::position].
    pub fn center(&self) -> Option<Point> {
        match self {
            Self::Node(_) => None,
            Self::Way(w) => w.center,
            Self::Relation(r) => r.center,
        }
    }

    /// Where this element is: the coordinates of a node, the [center](Element::center) of a way
    /// or a relation when the query asked for it.
    pub fn position(&self) -> Option<Point> {
        match self {
            Self::Node(n) => Some(Point { lat: n.lat, lon: n.lon }),
            _ => self.center(),
        }
    }

    /// The fields of this element that its type does not model (`bounds`, `geometry`,
    /// `timestamp`, `version`, `user`… depending on what the query asked for).
    pub fn other_fields(&self) -> &HashMap<String, serde_json::Value> {
        match self {
            Self::Node(n) => &n.other_fields,
            Self::Way(w) => &w.other_fields,
            Self::Relation(r) => &r.other_fields,
        }
    }
}

/// A node is one of the core elements in the OpenStreetMap data model.
///
/// [wiki](https://wiki.openstreetmap.org/wiki/Node)
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Node {
    pub id: i64,
    pub lat: f64,
    pub lon: f64,
    /// Empty when the node has no tags, or when the query did not ask for them.
    #[serde(default)]
    pub tags: HashMap<String, String>,
    /// Any other field the server sent.
    #[serde(flatten)]
    pub other_fields: HashMap<String, serde_json::Value>,
}

/// A way is one of the fundamental elements of the map.
///
/// [wiki](https://wiki.openstreetmap.org/wiki/Way)
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Way {
    pub id: i64,
    /// Empty when the way has no tags, or when the query did not ask for them.
    #[serde(default)]
    pub tags: HashMap<String, String>,
    /// Empty when the query did not ask for them (e.g. `out tags`).
    #[serde(default)]
    pub nodes: Vec<i64>,
    /// The center of the way's bounding box, when the query asked for it (`out center`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub center: Option<Point>,
    /// Any other field the server sent.
    #[serde(flatten)]
    pub other_fields: HashMap<String, serde_json::Value>,
}

/// Relations are structured collections of objects - nodes, ways, and other relations.
///
/// [wiki](https://wiki.openstreetmap.org/wiki/Relation)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Relation {
    pub id: i64,
    /// Empty when the relation has no tags, or when the query did not ask for them.
    #[serde(default)]
    pub tags: HashMap<String, String>,
    /// Empty when the query did not ask for them (e.g. `out tags`).
    #[serde(default)]
    pub members: Vec<RelationMember>,
    /// The center of the relation's bounding box, when the query asked for it (`out center`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub center: Option<Point>,
    /// Any other field the server sent.
    #[serde(flatten)]
    pub other_fields: HashMap<String, serde_json::Value>,
}

/// A reference to another [Node], [Way], or [Relation] from the owning relation, with an optional role.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RelationMember {
    #[serde(flatten)]
    pub id: ElementId,
    #[serde(deserialize_with = "skip_empty")]
    pub role: Option<String>,
}

fn skip_empty<'de, D>(deserializer: D) -> Result<Option<String>, D::Error> where D: Deserializer<'de> {
    deserializer.deserialize_string(OptionalStringVisitor)
}

struct OptionalStringVisitor;
impl<'de> Visitor<'de> for OptionalStringVisitor {
    type Value = Option<String>;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(formatter, "a string")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E> where E: serde::de::Error {
        if v.is_empty() {
            Ok(None)
        } else {
            Ok(Some(v.to_string()))
        }
    }

    fn visit_string<E>(self, v: String) -> Result<Self::Value, E> where E: serde::de::Error {
        if v.is_empty() {
            Ok(None)
        } else {
            Ok(Some(v))
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn id() {
        let a = ElementId::Node(123);
        let str = serde_json::to_string(&a).unwrap();
        assert_eq!(&str, r#"{"type":"node","ref":123}"#);
        let b: ElementId = serde_json::from_str(&str).unwrap();
        assert_eq!(a, b);
    }

    /// What `out center tags;` returns.
    const CENTER_TAGS: &str = r#"[
        {"type": "node", "id": 26864258, "lat": 45.8326, "lon": 6.8652,
         "tags": {"name": "Mont Blanc", "natural": "peak", "ele": "4806"}},
        {"type": "way", "id": 4005613, "center": {"lat": 45.9237, "lon": 6.8694},
         "tags": {"name": "Lac Blanc", "natural": "water"}},
        {"type": "relation", "id": 104005, "center": {"lat": 45.9, "lon": 6.9}},
        {"type": "node", "id": 1, "lat": 1.5, "lon": 2.5}
    ]"#;

    #[test]
    fn center_and_tags() {
        let elements: Vec<Element> = serde_json::from_str(CENTER_TAGS).unwrap();

        assert_eq!(elements[0].id(), ElementId::Node(26864258));
        assert_eq!(elements[0].tag("ele"), Some("4806"));
        assert_eq!(elements[0].center(), None);
        assert_eq!(elements[0].position(), Some(Point { lat: 45.8326, lon: 6.8652 }));

        assert_eq!(elements[1].id(), ElementId::Way(4005613));
        assert_eq!(elements[1].tag("name"), Some("Lac Blanc"));
        assert_eq!(elements[1].center(), Some(Point { lat: 45.9237, lon: 6.8694 }));
        assert_eq!(elements[1].position(), elements[1].center());
        assert!(matches!(&elements[1], Element::Way(w) if w.nodes.is_empty()));

        assert_eq!(elements[2].id(), ElementId::Relation(104005));
        assert_eq!(elements[2].position(), Some(Point { lat: 45.9, lon: 6.9 }));
        assert_eq!(elements[2].tags().len(), 0);
        assert!(matches!(&elements[2], Element::Relation(r) if r.members.is_empty()));

        assert_eq!(elements[3].tags().len(), 0);
    }

    #[test]
    fn body_without_center() {
        let elements: Vec<Element> = serde_json::from_str(r#"[
            {"type": "way", "id": 7, "nodes": [1, 2, 3], "tags": {"highway": "path"}},
            {"type": "relation", "id": 8, "tags": {"type": "route"}, "members": [
                {"type": "way", "ref": 7, "role": ""},
                {"type": "node", "ref": 1, "role": "stop"}
            ]}
        ]"#).unwrap();

        assert!(matches!(&elements[0], Element::Way(w) if w.nodes == [1, 2, 3]));
        assert_eq!(elements[0].position(), None);
        let Element::Relation(relation) = &elements[1] else { panic!("not a relation") };
        assert_eq!(relation.members, vec![
            RelationMember { id: ElementId::Way(7), role: None },
            RelationMember { id: ElementId::Node(1), role: Some("stop".to_string()) },
        ]);
    }

    #[test]
    fn unknown_fields_are_kept() {
        let json = serde_json::json!({
            "type": "way", "id": 7, "nodes": [1, 2], "tags": {"highway": "path"},
            "center": {"lat": 1.0, "lon": 2.0},
            "bounds": {"minlat": 0.5, "minlon": 1.5, "maxlat": 1.5, "maxlon": 2.5},
            "timestamp": "2024-01-01T00:00:00Z", "version": 3, "user": "someone",
        });
        let element: Element = serde_json::from_value(json.clone()).unwrap();

        assert_eq!(element.other_fields()["bounds"]["maxlat"], 1.5);
        assert_eq!(element.other_fields()["version"], 3);
        assert_eq!(element.other_fields()["user"], "someone");
        assert!(!element.other_fields().contains_key("center"));
        assert!(!element.other_fields().contains_key("type"));

        // Nothing is lost on the way back.
        assert_eq!(serde_json::to_value(&element).unwrap(), json);
    }
}
