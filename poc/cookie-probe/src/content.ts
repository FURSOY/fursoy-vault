const COOKIE_PROBE_DOCWRITE_NAME = "FCP-docwrite-diagnostic";

interface CookieProbeContentMessage {
  target?: unknown;
  command?: unknown;
}

chrome.runtime.onMessage.addListener((message: unknown, _sender, sendResponse) => {
  const request = message as CookieProbeContentMessage;
  if (request.target !== "fcp-cookie-probe") {
    return;
  }

  try {
    switch (request.command) {
      case "ping":
        sendResponse({ ok: true, data: { ready: true } });
        return;
      case "docwrite-diagnostic": {
        document.cookie = `${COOKIE_PROBE_DOCWRITE_NAME}=1; path=/`;
        const documentCookieNames = readCookieProbeDocumentNames();
        sendResponse({
          ok: true,
          data: {
            cookieName: COOKIE_PROBE_DOCWRITE_NAME,
            documentCookieNames,
            visibleInDocumentCookie: documentCookieNames.includes(COOKIE_PROBE_DOCWRITE_NAME),
          },
        });
        return;
      }
      case "docwrite-cleanup":
        document.cookie = `${COOKIE_PROBE_DOCWRITE_NAME}=; path=/; max-age=0`;
        sendResponse({
          ok: true,
          data: { cleaned: !readCookieProbeDocumentNames().includes(COOKIE_PROBE_DOCWRITE_NAME) },
        });
        return;
      default:
        sendResponse({ ok: false, error: `unknown command: ${String(request.command)}` });
        return;
    }
  } catch (error) {
    sendResponse({ ok: false, error: error instanceof Error ? error.message : String(error) });
  }
});

function readCookieProbeDocumentNames(): string[] {
  return document.cookie
    .split(";")
    .map((pair) => pair.trim())
    .filter((pair) => pair.length > 0)
    .map((pair) => {
      const separator = pair.indexOf("=");
      return separator < 0 ? pair : pair.slice(0, separator);
    });
}
