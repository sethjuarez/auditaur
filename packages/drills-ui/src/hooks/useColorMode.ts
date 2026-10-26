// Light or dark, following the host's `data-color-mode` (or its tone, or the OS when standalone).
// Host theme names vary, so the page always maps them onto Primer's own light and dark themes.
import { useSyncExternalStore } from "react";

export type ColorMode = "light" | "dark";

const DARK_QUERY = "(prefers-color-scheme: dark)";

function read(): ColorMode {
  const { colorMode, themeTone } = document.documentElement.dataset;
  for (const value of [colorMode, themeTone]) if (value === "light" || value === "dark") return value;
  return matchMedia(DARK_QUERY).matches ? "dark" : "light";
}

function subscribe(changed: () => void) {
  const observer = new MutationObserver(changed);
  observer.observe(document.documentElement, { attributes: true, attributeFilter: ["data-color-mode", "data-theme-tone"] });
  const media = matchMedia(DARK_QUERY);
  media.addEventListener("change", changed);
  return () => {
    observer.disconnect();
    media.removeEventListener("change", changed);
  };
}

export const useColorMode = () => useSyncExternalStore(subscribe, read);
