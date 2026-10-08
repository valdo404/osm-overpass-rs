# overpass-lib


Easily create and execute [Overpass API](https://wiki.openstreetmap.org/wiki/Overpass_API)
queries of [OpenStreetMap](https://wiki.openstreetmap.org/wiki/About_OpenStreetMap). Supports
both declarative and builder syntax for generating OverpassQL strings, a [reqwest]-based
submission API, and type definitions for handling the query results.

## Usage

Just like in OverpassQL, you specify a [Set] of [Element]s that you want, defined either as a basic [FilterSet]
or as compositions of other sets (e.g. [UnionSet]). You can either define these structs directly, or use
[SetBuilder] for more convenient syntax.

Once your set is specified, you:

1. Convert it to a [Query] or [QueryBuilder],
1. Apply any top-level settings to the query (e.g. [Query::search_bbox]),
1. Evaluate the query via an Overpass API server ([OverpassServer]),
1. Do whatever you like with the [Element]s returned!


## Examples

```rust
// Let's find all the landmarks in downtown Seattle in a standard OverpassQL string.
let oql = [
    "[bbox:47.553,-122.461,47.667,-122.201][out:json];",
    r#"nw["seamark:type"="landmark"];"#,
    "out;",
].join("");

// Using the declarative syntax, it looks like this:
let dec_query = Query {
    set: FilterSet {
        filter_type: FilterType::NodeOrWay,
        tag_filters: HashSet::from([TagFilter::equals("seamark:type", "landmark")]),
        ..Default::default()
    }.into(),
    search_bbox: Some(Bbox {
        north: 47.667,
        south: 47.553,
        east: -122.201,
        west: -122.461,
    }),
    ..Default::default()
};
assert_eq!(&oql, &dec_query.to_oql());

// Using the builder API it looks like this:
let builder_query: Query = SetBuilder::all_nodes_or_ways()
    .with_tag_value("seamark:type", "landmark")
    .to_query()
    .search_bbox(Bbox {
        north: 47.667,
        south: 47.553,
        east: -122.201,
        west: -122.461,
    })
    .into();
assert_eq!(&oql, &builder_query.to_oql());

// Evaluate the query via the default Overpass API server
let res = OverpassServer::default().evaluate(&dec_query).await.unwrap();

// One of those landmarks should be the Space Needle.
assert!(res.elements.iter().any(|e| matches!(e.tag("name"), Some("Space Needle"))));
```

## Raw OverpassQL and server settings

Whatever the query types cannot express yet can be sent as an OverpassQL string written by hand,
and the server, the HTTP client, the timeout and the request headers are yours to choose:

```rust
use reqwest::header::HeaderValue;

let server = OverpassServer::new("https://overpass-api.de/api/interpreter")
    .with_client(reqwest::Client::new())
    .with_timeout(Duration::from_secs(60))
    .with_user_agent(HeaderValue::from_static("my-app/1.0"));

let oql = r#"[out:json];relation["name"="Seattle"]["boundary"="administrative"];out center tags;"#;

// As elements: ways and relations carry their `center` when the query asks for it.
let res = server.evaluate_oql(oql).await.unwrap();
for element in &res.elements {
    println!("{:?} is at {:?}", element.tag("name"), element.position());
}

// Or as the JSON document the server sent.
let json = server.evaluate_oql_json(oql).await.unwrap();
println!("{}", json["elements"][0]["center"]);
```

## Cargo features

The default features are those of [reqwest] plus `deflate`. With `default-features = false` no TLS
backend is selected: enable `rustls` or `native-tls`, or let the rest of your application decide.

## Language Support

* Settings
    * ✅ timeout
    * ✅ maxsize
    * ✅ bbox
    * ✅ date
    * ✅ diff
    * ❌ adiff
    * ❌ out count
    * ✅ out verbosity
    * ❌ out modificators
    * ❌ out bbox
    * ❌ out sort order
    * ❌ out limit
* Sets
    * ✅ union
    * ❌ difference
    * ✅ intersection
    * ❌ if-block
    * ❌ foreach
    * ❌ for
    * ❌ complete
    * ❌ retro
    * ❌ compare
    * ❌ recurse up
    * ❌ recurse up relations
    * ❌ recurse down
    * ❌ recurse down relations
    * ❌ is_in
    * ❌ timeline
    * ❌ local
    * ❌ convert
    * ❌ make
    * Filters
        * ✅ has-kv
        * ✅ bbox
        * ✅ recurse refs
        * ❌ recurse way cnt/link
        * ✅ input set
        * ✅ id
        * ❌ around
        * ❌ poly
        * ❌ newer
        * ❌ changed
        * ❌ user
        * ❌ area
        * ❌ pivot
        * ❌ if
    * ❌ evaluators

## Contributing

Issues and pull requests welcome through
[GitHub](https://github.com/stevenvergenz/osm-overpass-rs).


License: MIT