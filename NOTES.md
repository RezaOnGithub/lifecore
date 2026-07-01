# TODO

- Investigate a mechanism to to "parse" node SmallTable by converting to HashMap, then JSON, then to another type
  - i.e. HashMap type itself should serialize transparently, inner Payload types should be transparent towards inner etc.

- Finish typechecking, and start testing by writing out "common" scenarios

- Change basic_validation so that MOCK events can reside at the end, and can be arbitrarily many (though emit a Warning that they are present.)

- Due to how fields are very tightly typecheched, having an array as a node type field fails miserably because serde can't just "magically" convert it into a string. So the course of action now is:
  - a node type has a " very external representation" (native types) and an "external representation" (PayloadExt)
  and performs conversion on its own.
  - main lifecore module converts from "external representation" to the weird internal thing I devised as premature optimization...


# Backlog

- Gradually phase out `anyhow` in favor of `thiserror`

- Clear choice of words regarding special features: Survivor is a node that is not Stale, Leaf is a node that has only exactly one outgoing edge

- Adding or subtracting from a Range's start and end is not really safe, but who cares?

# WishList

- `EnumAs` trait that panics, and only derivable for `Copy` enums
  - Usecase:
  ```rust
  vec_of_enum.iter().filter(|x| x.is_A()).map(|x| x.as_A())
  ```
- ... `strum` crate seems quite low-maintenance; I should fork it and go crazy
  with additions I want.
  - Better yet, I could use `facet` to implement them.
