// The last few errors on the page, kept in memory for "Report a problem".

const recent: string[] = [];

export function recordError(text: string) {
  const line = `${new Date().toISOString().slice(11, 19)} ${text}`.slice(0, 500);
  if (recent[recent.length - 1] === line) return;
  recent.push(line);
  if (recent.length > 30) recent.shift();
}

export function recentErrors(): string[] {
  return [...recent];
}

/** Catch uncaught errors and rejected promises (call once at startup). */
export function watchErrors() {
  window.addEventListener("error", (e) => {
    const where = e.filename ? ` (${e.filename.split("/").pop()?.split("?")[0]}:${e.lineno})` : "";
    recordError(`${e.message}${where}`);
  });
  window.addEventListener("unhandledrejection", (e) => {
    const r = e.reason as { message?: string } | string | undefined;
    recordError(`Unhandled: ${typeof r === "string" ? r : (r?.message ?? JSON.stringify(r))}`);
  });
}
