// Stand-ins for jsrt items the stubs use before crates/jsrt defines them. The compile check
// appends a section to its scratch copy of jsrt only when crates/jsrt lacks the item. Each item
// is a request to the jsrt owner; the real definition may differ as long as the name and the
// shape the stubs rely on stay.
