const EXPECTED_ORIGINS = new Set([
  "http://localhost:43118",
  "http://127.0.0.1:43118",
]);
const CONTENT_DUMMY_USERNAME = "probe-user";
const CONTENT_DUMMY_PASSWORD = "probe-password";
const CONTENT_DOCWRITE_DIAGNOSTIC_COOKIE_NAME = "FCP-docwrite-diagnostic";

type FirstPartyContentCommand =
  | "ping"
  | "login"
  | "protected"
  | "logout"
  | "diagnostics"
  | "page-diagnostic"
  | "docwrite-diagnostic"
  | "docwrite-cleanup"
  | "reset";

interface CommandMessage {
  target: "fcp-session-probe";
  command: FirstPartyContentCommand;
}

chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  if (!isCommandMessage(message)) {
    return;
  }

  void handleCommand(message.command)
    .then((data) => sendResponse({ ok: true, data }))
    .catch((error: unknown) => sendResponse({ ok: false, error: contentErrorMessage(error) }));
  return true;
});

async function handleCommand(command: FirstPartyContentCommand): Promise<unknown> {
  if (!EXPECTED_ORIGINS.has(window.location.origin)) {
    throw new Error(`unexpected content-script origin: ${window.location.origin}`);
  }

  switch (command) {
    case "ping":
      return { ready: true, origin: window.location.origin };
    case "login":
      return sameOriginRequest("/api/login", {
        method: "POST",
        body: JSON.stringify({
          username: CONTENT_DUMMY_USERNAME,
          password: CONTENT_DUMMY_PASSWORD,
        }),
      });
    case "protected":
      return sameOriginRequest("/api/protected");
    case "logout":
      return sameOriginRequest("/api/logout", { method: "POST", body: "{}" });
    case "diagnostics":
      return sameOriginRequest("/api/diagnostics");
    case "page-diagnostic":
      return {
        origin: window.location.origin,
        href: window.location.href,
        documentCookieNames: readDocumentCookieNames(),
      };
    case "docwrite-diagnostic": {
      document.cookie = `${CONTENT_DOCWRITE_DIAGNOSTIC_COOKIE_NAME}=1; path=/`;
      const documentCookieNames = readDocumentCookieNames();
      return {
        cookieName: CONTENT_DOCWRITE_DIAGNOSTIC_COOKIE_NAME,
        documentCookieNames,
        visibleInDocumentCookie: documentCookieNames.includes(
          CONTENT_DOCWRITE_DIAGNOSTIC_COOKIE_NAME,
        ),
      };
    }
    case "docwrite-cleanup":
      document.cookie = `${CONTENT_DOCWRITE_DIAGNOSTIC_COOKIE_NAME}=; path=/; max-age=0`;
      return {
        cleaned: !readDocumentCookieNames().includes(CONTENT_DOCWRITE_DIAGNOSTIC_COOKIE_NAME),
      };
    case "reset":
      return sameOriginRequest("/api/reset", { method: "POST", body: "{}" });
  }
}

async function sameOriginRequest(path: string, init: RequestInit = {}): Promise<unknown> {
  const response = await fetch(path, {
    ...init,
    cache: "no-store",
    credentials: "same-origin",
    headers: {
      "Content-Type": "application/json",
      ...init.headers,
    },
  });
  const body = (await response.json()) as unknown;
  if (!response.ok) {
    throw new Error(`${path} returned HTTP ${response.status}: ${JSON.stringify(body)}`);
  }
  return body;
}

function isCommandMessage(value: unknown): value is CommandMessage {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const candidate = value as Partial<CommandMessage>;
  return (
    candidate.target === "fcp-session-probe" &&
    typeof candidate.command === "string" &&
    [
      "ping",
      "login",
      "protected",
      "logout",
      "diagnostics",
      "page-diagnostic",
      "docwrite-diagnostic",
      "docwrite-cleanup",
      "reset",
    ].includes(candidate.command)
  );
}

function readDocumentCookieNames(): string[] {
  if (document.cookie.trim() === "") {
    return [];
  }
  return document.cookie
    .split(";")
    .map((pair) => {
      const separator = pair.indexOf("=");
      return (separator < 0 ? pair : pair.slice(0, separator)).trim();
    })
    .filter((name) => name.length > 0)
    .sort();
}

function contentErrorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
