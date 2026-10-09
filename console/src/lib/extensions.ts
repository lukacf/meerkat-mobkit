import React from "react";
import { consoleExtensionModuleUrl, validateConsoleExtensions, type ConsoleExtension, type ConsoleExtensionService } from "@console-core";

const EMPTY_EXTENSIONS: readonly ConsoleExtension[] = [];

/** Failed modules do not prevent independent extensions or stock views loading. */
export function useConsoleExtensions(
  supplied: readonly ConsoleExtension[] = EMPTY_EXTENSIONS,
  modules: readonly string[] = [],
  baseUrl = "",
  load: (url: string) => Promise<{ default: ConsoleExtension }> = loadModule,
) {
  const moduleKey = JSON.stringify(modules);
  const [loaded, setLoaded] = React.useState<{
    supplied: readonly ConsoleExtension[]; moduleKey: string; baseUrl: string;
    extensions: readonly ConsoleExtension[]; errors: string[];
  } | null>(null);
  const initial = React.useMemo(() => {
    try { validateConsoleExtensions(supplied); return { extensions: supplied, errors: [] as string[] }; }
    catch (error) { return { extensions: EMPTY_EXTENSIONS, errors: [String(error)] }; }
  }, [supplied]);
  React.useEffect(() => {
    let active = true;
    const paths: string[] = JSON.parse(moduleKey);
    void Promise.all(paths.map(async path => {
      try { return { extension: (await load(consoleExtensionModuleUrl(path, baseUrl))).default }; }
      catch (error) { return { error: `${path}: ${String(error)}` }; }
    })).then(results => {
      const extensions = [...initial.extensions], errors = [...initial.errors];
      for (const result of results) {
        if (result.error) { errors.push(result.error); continue; }
        try { validateConsoleExtensions([...extensions, result.extension!]); extensions.push(result.extension!); }
        catch (error) { errors.push(String(error)); }
      }
      if (active) setLoaded({ supplied, moduleKey, baseUrl, extensions, errors });
    });
    return () => { active = false; };
  }, [supplied, moduleKey, baseUrl, load, initial]);
  return loaded?.supplied === supplied && loaded.moduleKey === moduleKey && loaded.baseUrl === baseUrl ? loaded : initial;
}

function loadModule(url: string): Promise<{ default: ConsoleExtension }> {
  return import(/* @vite-ignore */ url);
}

/** No credentials are handed to plugins, and requests are never replayed. */
export function createConsoleExtensionService(baseUrl: string): ConsoleExtensionService {
  return async (request, scope, signal) => {
    const method = request.method ?? "GET";
    if (scope.readOnly && method !== "GET") throw new Error("This console is view only");
    const url = consoleExtensionModuleUrl(request.path, baseUrl);
    const response = await fetch(url, { method, signal, credentials: "same-origin", redirect: "error",
      ...(request.body === undefined ? {} : { headers: { "Content-Type": "application/json" }, body: JSON.stringify(request.body) }),
    });
    if (!response.ok) throw new Error(`Extension request failed (${response.status})`);
    return response.status === 204 ? null : response.json();
  };
}

export function bindConsoleExtensionService(service: ConsoleExtensionService, authoritySignal: AbortSignal): ConsoleExtensionService {
  return async (request, scope, signal) => {
    if (scope.readOnly && (request.method ?? "GET") !== "GET") throw new Error("This console is view only");
    const combined = new AbortController();
    const abort = () => combined.abort();
    authoritySignal.addEventListener("abort", abort, { once: true });
    signal.addEventListener("abort", abort, { once: true });
    try {
      if (authoritySignal.aborted || signal.aborted) combined.abort();
      combined.signal.throwIfAborted();
      const result = await service(request, scope, combined.signal);
      combined.signal.throwIfAborted();
      return result;
    } finally {
      authoritySignal.removeEventListener("abort", abort);
      signal.removeEventListener("abort", abort);
    }
  };
}
