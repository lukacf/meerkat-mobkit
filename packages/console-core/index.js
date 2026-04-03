"use strict";

const crypto = require("node:crypto");

function rpcId(prefix = "rpc") {
  return `${prefix}:${crypto.randomUUID()}`;
}

function normalizeBaseUrl(baseUrl) {
  return String(baseUrl || "").replace(/\/+$/, "");
}

async function fetchJson(baseUrl, route, options = {}) {
  const response = await fetch(`${normalizeBaseUrl(baseUrl)}${route}`, {
    method: options.method || "GET",
    headers: {
      ...(options.headers || {}),
    },
    body: options.body,
    signal: options.signal,
  });

  if (!response.ok) {
    const text = await response.text().catch(() => "");
    throw new Error(`${options.method || "GET"} ${route} failed ${response.status}: ${text}`);
  }

  return response.json();
}

async function jsonRpc(baseUrl, method, params = {}, options = {}) {
  const response = await fetch(`${normalizeBaseUrl(baseUrl)}${options.route || "/console/rpc"}`, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      ...(options.headers || {}),
    },
    body: JSON.stringify({
      jsonrpc: "2.0",
      id: options.id || rpcId(method),
      method,
      params,
    }),
    signal: options.signal,
  });

  if (!response.ok) {
    const text = await response.text().catch(() => "");
    throw new Error(`${method} failed ${response.status}: ${text}`);
  }

  const payload = await response.json();
  if (payload?.error) {
    throw new Error(payload.error.message || JSON.stringify(payload.error));
  }
  return payload?.result ?? null;
}

function createSseParser(onFrame) {
  let buffer = "";

  function flushBlock(block) {
    const trimmed = block.trim();
    if (!trimmed) {
      return;
    }
    const lines = trimmed.split(/\r?\n/);
    let id = "";
    let event = "message";
    const dataLines = [];

    for (const line of lines) {
      if (line.startsWith(":")) {
        continue;
      }
      if (line.startsWith("id:")) {
        id = line.slice(3).trim();
        continue;
      }
      if (line.startsWith("event:")) {
        event = line.slice(6).trim();
        continue;
      }
      if (line.startsWith("data:")) {
        dataLines.push(line.slice(5).trim());
      }
    }

    const rawData = dataLines.join("\n");
    let data = rawData;
    if (rawData) {
      try {
        data = JSON.parse(rawData);
      } catch {
        data = rawData;
      }
    }

    onFrame({
      id,
      event,
      data,
      rawData,
    });
  }

  return {
    push(chunk) {
      buffer += chunk;
      const parts = buffer.split(/\r?\n\r?\n/);
      buffer = parts.pop() || "";
      for (const part of parts) {
        flushBlock(part);
      }
    },
    end() {
      if (buffer.trim()) {
        flushBlock(buffer);
      }
      buffer = "";
    },
  };
}

async function readSse(response, onFrame) {
  if (!response.ok) {
    const text = await response.text().catch(() => "");
    throw new Error(`SSE request failed ${response.status}: ${text}`);
  }

  if (!response.body) {
    throw new Error("SSE response body missing");
  }

  const parser = createSseParser(onFrame);
  const reader = response.body.getReader();
  const decoder = new TextDecoder();

  while (true) {
    const { value, done } = await reader.read();
    if (done) {
      break;
    }
    parser.push(decoder.decode(value, { stream: true }));
  }

  parser.end();
}

async function openSse(baseUrl, route, options = {}, onFrame) {
  const response = await fetch(`${normalizeBaseUrl(baseUrl)}${route}`, {
    method: options.method || "GET",
    headers: {
      ...(options.headers || {}),
    },
    body: options.body,
    signal: options.signal,
  });
  return readSse(response, onFrame);
}

module.exports = {
  createSseParser,
  fetchJson,
  jsonRpc,
  normalizeBaseUrl,
  openSse,
  readSse,
  rpcId,
};
