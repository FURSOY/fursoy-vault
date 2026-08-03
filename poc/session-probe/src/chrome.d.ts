declare namespace chrome {
  namespace runtime {
    const lastError: { message?: string } | undefined;
    function getURL(path: string): string;
    function sendMessage(message: unknown, callback: (response: unknown) => void): void;

    interface MessageSender {
      tab?: tabs.Tab;
    }

    const onMessage: {
      addListener(
        callback: (
          message: unknown,
          sender: MessageSender,
          sendResponse: (response: unknown) => void,
        ) => boolean | void,
      ): void;
    };

    const onStartup: {
      addListener(callback: () => void): void;
    };
  }

  namespace action {
    const onClicked: {
      addListener(callback: () => void): void;
    };
  }

  namespace tabs {
    interface Tab {
      active?: boolean;
      id?: number;
      incognito: boolean;
      status?: "loading" | "complete";
      url?: string;
      windowId: number;
    }

    interface TabChangeInfo {
      status?: "loading" | "complete";
      url?: string;
    }

    function create(
      createProperties: { url: string; active?: boolean },
      callback?: (tab: Tab) => void,
    ): void;
    function getCurrent(callback: (tab?: Tab) => void): void;
    function get(tabId: number, callback: (tab: Tab) => void): void;
    function query(
      queryInfo: { active?: boolean; lastFocusedWindow?: boolean; url?: string[] },
      callback: (tabs: Tab[]) => void,
    ): void;
    function sendMessage(tabId: number, message: unknown, callback: (response: unknown) => void): void;
    function remove(tabId: number, callback?: () => void): void;

    const onActivated: {
      addListener(callback: (activeInfo: { tabId: number; windowId: number }) => void): void;
    };
    const onRemoved: {
      addListener(callback: (tabId: number, removeInfo: { isWindowClosing: boolean; windowId: number }) => void): void;
    };
    const onUpdated: {
      addListener(callback: (tabId: number, changeInfo: TabChangeInfo, tab: Tab) => void): void;
    };
  }

  namespace windows {
    interface Window {
      focused: boolean;
      id?: number;
      tabs?: tabs.Tab[];
    }

    const WINDOW_ID_NONE: number;
    function getLastFocused(
      getInfo: { populate?: boolean },
      callback: (window: Window) => void,
    ): void;
    const onFocusChanged: {
      addListener(callback: (windowId: number) => void): void;
    };
  }

  namespace idle {
    type IdleState = "active" | "idle" | "locked";
    function queryState(detectionIntervalInSeconds: number, callback: (state: IdleState) => void): void;
    function setDetectionInterval(intervalInSeconds: number): void;
    const onStateChanged: {
      addListener(callback: (newState: IdleState) => void): void;
    };
  }

  namespace storage {
    interface StorageArea {
      get(keys: string | string[] | null, callback: (items: Record<string, unknown>) => void): void;
      set(items: Record<string, unknown>, callback?: () => void): void;
      remove(keys: string | string[], callback?: () => void): void;
    }

    const session: StorageArea;
  }

  namespace alarms {
    interface Alarm {
      name: string;
      scheduledTime: number;
    }

    function create(name: string, alarmInfo: { when: number }): void;
    function clear(name: string, callback?: (wasCleared: boolean) => void): void;
    const onAlarm: {
      addListener(callback: (alarm: Alarm) => void): void;
    };
  }

  namespace cookies {
    type SameSiteStatus = "no_restriction" | "lax" | "strict" | "unspecified";

    interface CookiePartitionKey {
      topLevelSite?: string;
      hasCrossSiteAncestor?: boolean;
    }

    interface Cookie {
      domain: string;
      expirationDate?: number;
      hostOnly: boolean;
      httpOnly: boolean;
      name: string;
      partitionKey?: CookiePartitionKey;
      path: string;
      sameSite: SameSiteStatus;
      secure: boolean;
      session: boolean;
      storeId: string;
      value: string;
    }

    interface CookieStore {
      id: string;
      tabIds: number[];
    }

    interface CookieSetDetails {
      url: string;
      name?: string;
      value?: string;
      domain?: string;
      path?: string;
      secure?: boolean;
      httpOnly?: boolean;
      sameSite?: SameSiteStatus;
      expirationDate?: number;
      storeId?: string;
      partitionKey?: CookiePartitionKey;
    }

    interface CookieDetails {
      url: string;
      name: string;
      storeId?: string;
      partitionKey?: CookiePartitionKey;
    }

    interface GetAllDetails {
      url?: string;
      name?: string;
      storeId?: string;
      partitionKey?: CookiePartitionKey;
    }

    function get(details: CookieDetails, callback: (cookie: Cookie | null) => void): void;
    function getAll(details: GetAllDetails, callback: (cookies: Cookie[]) => void): void;
    function set(details: CookieSetDetails, callback: (cookie?: Cookie) => void): void;
    function remove(
      details: CookieDetails,
      callback: (details: { name: string; storeId: string; url: string } | null) => void,
    ): void;
    function getAllCookieStores(callback: (stores: CookieStore[]) => void): void;
    const onChanged: {
      addListener(
        callback: (changeInfo: { removed: boolean; cookie: Cookie; cause: string }) => void,
      ): void;
    };
  }
}
