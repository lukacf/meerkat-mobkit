import { createConsoleApp } from "../src/index";
import "../src/console-host.css";
import "@console-components/styles";
import { acceptanceMarkdownUrlPolicy } from "./markdown-url-policy";

// The local test host owns this namespace. Production hosts must supply their
// authenticated authority/realm/principal namespace through the public API.
createConsoleApp(document.getElementById("root"), {
  baseUrl: location.origin,
  storageNamespace: `${location.origin}/acceptance-realm/operator-a`,
  markdownUrlPolicy: acceptanceMarkdownUrlPolicy(),
});
