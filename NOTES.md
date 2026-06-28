# TODO

- Gradually phase out `anyhow` in favor of `thiserror`

# WishList

- `EnumAs` trait that panics, and only derivable for `Copy` enums
  - Usecase:
  ```rust
  vec_of_enum.iter(|x| x.is_A()).map(|x| x.as_A())
  ```
- ... `strum` crate seems quite low-maintenance; I should fork it and go crazy
  with additions I want.
  - Better yet, I could use `facet` to implement them.
