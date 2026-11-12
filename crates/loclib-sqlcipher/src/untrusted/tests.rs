use super::*;

fn database(sql: &str) -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(sql).unwrap();
    conn
}

#[test]
fn a_schema_object_the_maker_never_makes_is_named() {
    let conn = database("CREATE TABLE t (a); CREATE VIEW v AS SELECT a FROM t;");
    assert_eq!(distrust(&conn, &["trigger"], "rekordbox").unwrap(), None);
    assert_eq!(
        distrust(&conn, &["trigger", "view"], "rekordbox").unwrap().as_deref(),
        Some("it has a view v, which rekordbox never makes")
    );
}

#[test]
fn a_distrusted_schema_may_not_call_functions_with_side_effects() {
    let conn = database("CREATE TABLE t (a);");
    distrust(&conn, &[], "rekordbox").unwrap();
    let trusted: i64 = conn.query_row("PRAGMA trusted_schema", [], |r| r.get(0)).unwrap();
    let checked: i64 = conn.query_row("PRAGMA cell_size_check", [], |r| r.get(0)).unwrap();
    assert_eq!((trusted, checked), (0, 1));
    // Defensive mode refuses writes to the schema table itself.
    assert!(conn.execute_batch("PRAGMA writable_schema = ON; DELETE FROM sqlite_master;").is_err());
}

#[test]
fn an_intact_database_is_not_damaged() {
    let conn = database("CREATE TABLE t (a); INSERT INTO t VALUES (1), (2);");
    assert_eq!(damaged(&conn).unwrap(), None);
}
