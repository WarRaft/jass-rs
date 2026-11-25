//! What a vJASS struct `extends array` can't have or do, said the same way
//! by the editor (`lint`, `names`: the rule `array-struct`) and the build
//! (`wc3-build`). Such a struct's instances are only numbers, used as they
//! are (`S[i]`): nothing creates or destroys them.

/// ` (from module M)` when a module gave it.
fn from(module: Option<&str>) -> String {
    module.map_or(String::new(), |m| format!(" (from module {m})"))
}

/// A member with a default value.
pub fn default_value(s: &str, member: &str, module: Option<&str>) -> String {
    format!(
        "{s} extends array: its member '{member}'{} can't have a default value. Its instances are only numbers and are never created, so nothing would set it: it starts as 0, null or false",
        from(module)
    )
}

/// A member that's an array.
pub fn array_member(s: &str, member: &str, module: Option<&str>) -> String {
    format!(
        "{s} extends array: its member '{member}'{} can't be an array. Each member is itself one array indexed by the instance, with no room for an array per instance",
        from(module)
    )
}

/// `onDestroy`.
pub fn on_destroy(s: &str, module: Option<&str>) -> String {
    format!("{s} extends array: onDestroy{} would never run. Its instances are only numbers: nothing creates or destroys them", from(module))
}

/// A call of `allocate`, `create`, `destroy` or `deallocate` it has none of.
pub fn call(s: &str, m: &str) -> String {
    match m {
        "allocate" | "create" => format!(
            "{s}.{m}: {s} extends array, so it has no {m}. Its instances are numbers used as they are ({s}[i]), never allocated; to make some, declare a static method {m} of its own"
        ),
        _ => format!(
            "{s}.{m}: {s} extends array, so it has no {m}. Its instances are numbers that were never allocated, so there's nothing to give back; to have one, declare a method {m} of its own"
        ),
    }
}
