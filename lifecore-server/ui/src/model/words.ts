// An English plural good enough for type names and the client's own nouns:
// Tasks, Addresses, Stories, inconsistencies. A count may be a decimal
// string, as entry counts are.
export function plural(noun: string, count: number | string): string {
  if (String(count) === "1") {
    return noun;
  }
  if (/(s|x|z|ch|sh)$/.test(noun)) {
    return `${noun}es`;
  }
  if (/[^aeiou]y$/.test(noun)) {
    return `${noun.slice(0, -1)}ies`;
  }
  return `${noun}s`;
}

// "1 warning", "3 inconsistencies", "460 Tasks".
export function counted(count: number | string, noun: string): string {
  return `${count} ${plural(noun, count)}`;
}
