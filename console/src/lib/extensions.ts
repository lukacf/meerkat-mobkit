import React from "react";
import { consoleExtensionModuleUrl, validateConsoleExtensions, type ConsoleExtension } from "@console-core";

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
