// One JSON-RPC call per HTTP request to `POST /rpc`, carrying the launch
// token. Failures are typed so the page can tell a JSON-RPC error from an
// access rejection or a lost connection.
import type {
  ErrorObject,
  HttpRejection,
  Method,
  Params,
  Result,
  RpcResponse,
} from "./contract.ts";

// The server answered the call with a JSON-RPC error object.
export class RpcError extends Error {
  readonly method: Method;
  readonly error: ErrorObject;
  constructor(method: Method, error: ErrorObject) {
    super(`${method} failed with ${error.code}: ${error.message}`);
    this.method = method;
    this.error = error;
  }
}

// The server refused the HTTP request before dispatch. `rejection` is null
// when the body was not a structured rejection.
export class HttpRejectionError extends Error {
  readonly status: number;
  readonly rejection: HttpRejection | null;
  constructor(status: number, rejection: HttpRejection | null, body: string) {
    super(rejection?.message ?? `HTTP ${status}: ${body}`);
    this.status = status;
    this.rejection = rejection;
  }
}

// No usable answer arrived. A write may or may not have been applied; it is
// never retried automatically.
export class TransportError extends Error {}

export type CallFailure = RpcError | HttpRejectionError | TransportError;

function parseRejection(text: string): HttpRejection | null {
  try {
    const body: unknown = JSON.parse(text);
    if (
      typeof body === "object" &&
      body !== null &&
      "kind" in body &&
      "message" in body &&
      typeof body.kind === "string" &&
      typeof body.message === "string"
    ) {
      return body as HttpRejection;
    }
  } catch {
    // Not JSON: report the status and text instead.
  }
  return null;
}

export class RpcClient {
  readonly #token: string | null;
  readonly #fetch: typeof fetch;
  readonly #endpoint: string;
  #nextId = 1;

  constructor(
    token: string | null,
    fetchImpl: typeof fetch = globalThis.fetch.bind(globalThis),
    endpoint = "/rpc",
  ) {
    this.#token = token;
    this.#fetch = fetchImpl;
    this.#endpoint = endpoint;
  }

  async call<M extends Method>(
    method: M,
    params: Params<M>,
  ): Promise<Result<M>> {
    const id = this.#nextId++;
    const headers: Record<string, string> = {
      "Content-Type": "application/json",
    };
    if (this.#token !== null) {
      headers.Authorization = `Bearer ${this.#token}`;
    }
    let response: Response;
    try {
      response = await this.#fetch(this.#endpoint, {
        method: "POST",
        headers,
        body: JSON.stringify({ jsonrpc: "2.0", id, method, params }),
      });
    } catch (error) {
      throw new TransportError(`cannot reach the server: ${String(error)}`);
    }
    const text = await response.text();
    if (!response.ok) {
      throw new HttpRejectionError(response.status, parseRejection(text), text);
    }
    let body: RpcResponse<M>;
    try {
      body = JSON.parse(text);
    } catch {
      throw new TransportError("the server's answer is not JSON");
    }
    if ("error" in body) {
      throw new RpcError(method, body.error);
    }
    if (body.id !== id) {
      throw new TransportError("the server's answer does not match this call");
    }
    return body.result;
  }
}
