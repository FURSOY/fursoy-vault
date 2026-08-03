declare namespace chrome {
  namespace runtime {
    const lastError: { message?: string } | undefined;
    function getURL(path: string): string;

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
  }

  namespace action {
    const onClicked: {
      addListener(callback: () => void): void;
    };
  }

  namespace tabs {
    interface Tab {
      id?: number;
      incognito: boolean;
      url?: string;
      windowId: number;
    }

    function create(
      createProperties: { url: string; active?: boolean },
      callback?: (tab: Tab) => void,
    ): void;
    function getCurrent(callback: (tab?: Tab) => void): void;
    function get(tabId: number, callback: (tab: Tab) => void): void;
    function sendMessage(tabId: number, message: unknown, callback: (response: unknown) => void): void;
    function remove(tabId: number, callback?: () => void): void;
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
  }
}
