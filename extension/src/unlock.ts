type UnlockStatus = "ready" | "unlocking" | "recovering" | "redirecting" | "error";

interface UnlockResponse {
  ok?: boolean;
  status?: UnlockStatus;
  error?: string;
}

const button = requiredElement<HTMLButtonElement>("unlock");
const statusText = requiredElement<HTMLElement>("status");
let requestActive = false;
let pollInFlight = false;

button.addEventListener("click", () => {
  requestActive = true;
  render("unlocking");
  sendUnlockMessage("unlock.start").then(handleResponse, () => {
    requestActive = false;
    render("error");
  });
});

setInterval(() => {
  if (pollInFlight) return;
  pollInFlight = true;
  sendUnlockMessage("unlock.status")
    .then(handleResponse, () => {
      requestActive = false;
      render("error");
    })
    .finally(() => { pollInFlight = false; });
}, 300);

void sendUnlockMessage("unlock.status").then(handleResponse, () => render("error"));

function handleResponse(response: unknown): void {
  const result = response as UnlockResponse | undefined;
  const status = result?.status ?? "error";
  if (status === "ready" || status === "error") requestActive = false;
  render(status);
}

function render(status: UnlockStatus): void {
  switch (status) {
    case "ready":
      statusText.textContent = "Windows Hello ile korunan cookie'leri açabilirsiniz.";
      button.disabled = false;
      break;
    case "unlocking":
      statusText.textContent = "Windows Hello onayı ve cookie enjeksiyonu bekleniyor…";
      button.disabled = true;
      break;
    case "recovering":
      statusText.textContent = "Güvenli durum geri yükleniyor; birazdan tekrar deneyebilirsiniz.";
      button.disabled = true;
      break;
    case "redirecting":
      statusText.textContent = "Cookie'ler hazır; site açılıyor…";
      button.disabled = true;
      break;
    case "error":
      statusText.textContent = "Oturum açılamadı. Yeniden deneyebilirsiniz.";
      button.disabled = requestActive;
      break;
  }
}

function sendUnlockMessage(type: "unlock.status" | "unlock.start"): Promise<unknown> {
  return new Promise((resolve, reject) => {
    chrome.runtime.sendMessage({ type }, (response) => {
      const error = chrome.runtime.lastError;
      error === undefined ? resolve(response) : reject(new Error("extension message failed"));
    });
  });
}

function requiredElement<T extends HTMLElement>(id: string): T {
  const element = document.getElementById(id);
  if (element === null) throw new Error(`missing required element ${id}`);
  return element as T;
}

export {};
