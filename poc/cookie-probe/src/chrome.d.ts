declare namespace chrome {
  namespace runtime {
    const lastError: { message?: string } | undefined;
    function getURL(path: string): string;
  }

  namespace action {
    const onClicked: {
      addListener(callback: () => void): void;
    };
  }

  namespace tabs {
    interface Tab {
      id?: number;
    }

    function create(createProperties: { url: string }): void;
    function getCurrent(callback: (tab?: Tab) => void): void;
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

    function get(details: CookieDetails, callback: (cookie: Cookie | null) => void): void;
    function set(details: CookieSetDetails, callback: (cookie?: Cookie) => void): void;
    function remove(
      details: CookieDetails,
      callback: (details: { name: string; storeId: string; url: string } | null) => void,
    ): void;
    function getAllCookieStores(callback: (stores: CookieStore[]) => void): void;
  }
}
