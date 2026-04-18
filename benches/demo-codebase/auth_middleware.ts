export interface AuthConfig {
  maxRetries: number;
  tokenExpiryMs: number;
  refreshThreshold: number;
}

export class AuthMiddleware {
  private config: AuthConfig;
  private tokenCache: Map<string, { token: string; expiresAt: number }>;

  constructor(config: AuthConfig) {
    this.config = config;
    this.tokenCache = new Map();
  }

  async validateSession(sessionId: string): Promise<boolean> {
    const cached = this.tokenCache.get(sessionId);
    if (!cached) return false;
    if (Date.now() > cached.expiresAt) {
      this.tokenCache.delete(sessionId);
      return false;
    }
    return true;
  }

  async refreshAccessToken(sessionId: string): Promise<string> {
    const newToken = `${sessionId}_${Date.now()}_refreshed`;
    this.tokenCache.set(sessionId, {
      token: newToken,
      expiresAt: Date.now() + this.config.tokenExpiryMs,
    });
    return newToken;
  }

  revokeSession(sessionId: string): void {
    this.tokenCache.delete(sessionId);
  }
}

export function parseConfigYaml(input: string): Record<string, unknown> {
  const result: Record<string, unknown> = {};
  for (const line of input.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("#")) continue;
    const colonIdx = trimmed.indexOf(":");
    if (colonIdx === -1) continue;
    const key = trimmed.slice(0, colonIdx).trim();
    const value = trimmed.slice(colonIdx + 1).trim();
    if (value === "true") {
      result[key] = true;
    } else if (value === "false") {
      result[key] = false;
    } else if (!isNaN(Number(value))) {
      result[key] = Number(value);
    } else {
      result[key] = value;
    }
  }
  return result;
}

export function sanitizeUserInput(input: string): string {
  return input
    .replace(/[<>"'&]/g, (ch) => {
      const map: Record<string, string> = {
        "<": "&lt;",
        ">": "&gt;",
        '"': "&quot;",
        "'": "&#x27;",
        "&": "&amp;",
      };
      return map[ch] || ch;
    });
}

export function serializeToJson(obj: unknown): string {
  return JSON.stringify(obj, null, 2);
}
