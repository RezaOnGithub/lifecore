export function withMember<K, V>(
  map: ReadonlyMap<K, V>,
  key: K,
  value: V | null,
): ReadonlyMap<K, V> {
  const copy = new Map(map);
  if (value === null) {
    copy.delete(key);
  } else {
    copy.set(key, value);
  }
  return copy;
}

export function withItem<T>(
  set: ReadonlySet<T>,
  item: T,
  member: boolean,
): ReadonlySet<T> {
  const copy = new Set(set);
  if (member) {
    copy.add(item);
  } else {
    copy.delete(item);
  }
  return copy;
}
