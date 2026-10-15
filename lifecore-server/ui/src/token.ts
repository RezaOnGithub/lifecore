// The launch token arrives once, in the URL fragment printed by
// `lifecore serve` (INTERFACES §3.2). It is kept in session storage, so a
// refresh keeps working, and removed from the address bar.

export const TOKEN_KEY = "lifecore.launchToken";

export interface TokenSources {
  location: Pick<Location, "hash" | "pathname" | "search">;
  history: Pick<History, "replaceState">;
  storage: Pick<Storage, "getItem" | "setItem">;
}

export function takeLaunchToken({
  location,
  history,
  storage,
}: TokenSources): string | null {
  const fragment = new URLSearchParams(location.hash.replace(/^#/, ""));
  const token = fragment.get("token");
  if (!token) {
    return storage.getItem(TOKEN_KEY);
  }
  storage.setItem(TOKEN_KEY, token);
  fragment.delete("token");
  const rest = fragment.toString();
  history.replaceState(
    null,
    "",
    `${location.pathname}${location.search}${rest ? `#${rest}` : ""}`,
  );
  return token;
}
