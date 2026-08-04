declare namespace chrome {
  namespace runtime {
    const id: string;
    const lastError: { message?: string } | undefined;
    function getURL(path: string): string;
    interface Port { postMessage(message: unknown): void; disconnect(): void;
      onMessage: { addListener(callback: (message: unknown) => void): void };
      onDisconnect: { addListener(callback: () => void): void }; }
    function connectNative(name: string): Port;
    function sendMessage(message: unknown, callback?: (response: unknown) => void): void;
    interface MessageSender { tab?: tabs.Tab }
    const onMessage: { addListener(callback: (message: unknown, sender: MessageSender, respond: (value: unknown) => void) => boolean | void): void };
    const onStartup: { addListener(callback: () => void): void };
  }
  namespace tabs {
    interface Tab { id?: number; url?: string; status?: "loading" | "complete"; active?: boolean }
    function query(query: { url?: string[] }, callback: (tabs: Tab[]) => void): void;
    function get(tabId: number, callback: (tab: Tab) => void): void;
    function update(tabId: number, properties: { url: string }, callback: (tab: Tab) => void): void;
    function sendMessage(tabId: number, message: unknown, callback: (response: unknown) => void): void;
    const onRemoved: { addListener(callback: (tabId: number) => void): void };
    const onUpdated: { addListener(callback: (tabId: number, change: { status?: string; url?: string }, tab: Tab) => void): void };
  }
  namespace webNavigation {
    interface NavigationDetails { tabId: number; frameId: number; url: string }
    const onBeforeNavigate: { addListener(callback: (details: NavigationDetails) => void): void };
  }
  namespace idle {
    type IdleState = "active" | "idle" | "locked";
    function setDetectionInterval(seconds: number): void;
    function queryState(seconds: number, callback: (state: IdleState) => void): void;
    const onStateChanged: { addListener(callback: (state: IdleState) => void): void };
  }
  namespace storage {
    interface Area { get(key: string, callback: (items: Record<string, unknown>) => void): void; set(items: Record<string, unknown>, callback?: () => void): void }
    const session: Area;
  }
  namespace alarms {
    interface Alarm { name: string; scheduledTime: number }
    function create(name: string, info: { when: number }): void;
    function clear(name: string, callback?: (cleared: boolean) => void): void;
    const onAlarm: { addListener(callback: (alarm: Alarm) => void): void };
  }
  namespace cookies {
    type SameSiteStatus = "no_restriction" | "lax" | "strict" | "unspecified";
    interface CookiePartitionKey { topLevelSite?: string; hasCrossSiteAncestor?: boolean }
    interface Cookie { domain: string; expirationDate?: number; hostOnly: boolean; httpOnly: boolean; name: string; partitionKey?: CookiePartitionKey; path: string; sameSite: SameSiteStatus; secure: boolean; session: boolean; storeId: string; value: string }
    interface SetDetails { url: string; name?: string; value?: string; domain?: string; path?: string; secure?: boolean; httpOnly?: boolean; sameSite?: SameSiteStatus; expirationDate?: number; storeId?: string; partitionKey?: CookiePartitionKey }
    function getAll(details: { url?: string; name?: string }, callback: (cookies: Cookie[]) => void): void;
    function set(details: SetDetails, callback: (cookie?: Cookie) => void): void;
    function remove(details: { url: string; name: string; storeId?: string; partitionKey?: CookiePartitionKey }, callback: (result: unknown) => void): void;
    const onChanged: { addListener(callback: (info: { removed: boolean; cookie: Cookie; cause: string }) => void): void };
  }
  namespace scripting {
    interface InjectionResult<T> { frameId: number; result?: T }
    function executeScript<T>(
      injection: { target: { tabId: number }; world?: "ISOLATED" | "MAIN"; func: () => T | Promise<T> },
      callback: (results: InjectionResult<T>[]) => void,
    ): void;
  }
}
