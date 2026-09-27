import { memo, useMemo, type ReactNode } from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";

import type { ConversationRichMarkdownBlock } from "@console-core";

/** URL decisions are presentation policy. Resolvers must not fetch or grant access. */
export interface MarkdownUrlPolicy {
  resolveLink?: (url: string) => string | null;
  resolveImage?: (url: string) => string | null;
}

function cleanUrl(url: string): string | null {
  const value = url.trim();
  if (!value || /[\u0000-\u001f\u007f]/u.test(value) || value.startsWith("//")) return null;
  if (/^(?:javascript|vbscript|data|file):/iu.test(value)) return null;
  return value;
}

export function resolveMarkdownLink(url: string, policy?: MarkdownUrlPolicy): string | null {
  const source = cleanUrl(url);
  if (!source) return null;
  const resolved = policy?.resolveLink ? policy.resolveLink(source) : source;
  if (resolved === null) return null;
  const value = cleanUrl(resolved);
  if (!value) return null;
  if (/^(?:https?:\/\/|mailto:|#)/iu.test(value)) return value;
  // Relative and app links require an explicit host resolver. Its output still
  // cannot introduce executable or unrecognized schemes.
  if (policy?.resolveLink && !/^[a-z][a-z\d+.-]*:/iu.test(value)) return value;
  return null;
}

export function resolveMarkdownImage(url: string, policy?: MarkdownUrlPolicy): string | null {
  const source = cleanUrl(url);
  if (!source || !policy?.resolveImage) return null;
  const resolved = policy.resolveImage(source);
  if (resolved === null) return null;
  const value = cleanUrl(resolved);
  if (!value) return null;
  return /^(?:https?:\/\/|blob:)/iu.test(value) || !/^[a-z][a-z\d+.-]*:/iu.test(value) ? value : null;
}

function sourcePosition(node: { position?: { start: { offset?: number }; end: { offset?: number } } } | undefined) {
  return {
    "data-source-start": node?.position?.start.offset,
    "data-source-end": node?.position?.end.offset,
  };
}

const plugins = [remarkGfm];

function isJsonDocument(source: string): boolean {
  if (!/^\s*[\[{]/u.test(source)) return false;
  try { return typeof JSON.parse(source) === "object"; } catch { return false; }
}

export interface ConversationMarkdownProps {
  block: ConversationRichMarkdownBlock;
  urlPolicy?: MarkdownUrlPolicy;
  className?: string;
}

export const ConversationMarkdown = memo(function ConversationMarkdown({ block, urlPolicy, className }: ConversationMarkdownProps) {
  const components = useMemo<Components>(() => ({
    a({ href = "", children, title, node, ...props }) {
      const url = resolveMarkdownLink(href, urlPolicy);
      if (!url) return <span {...sourcePosition(node)}>{children}</span>;
      const external = /^https?:\/\//iu.test(url);
      return <a {...props} href={url} title={title} {...sourcePosition(node)} {...(external ? { target: "_blank", rel: "noopener noreferrer" } : {})}>{children}</a>;
    },
    img({ src = "", alt = "", title, node }) {
      const url = resolveMarkdownImage(typeof src === "string" ? src : "", urlPolicy);
      return url
        ? <img src={url} alt={alt} title={title} loading="lazy" {...sourcePosition(node)} />
        : <span className="cc-markdown-image-placeholder" {...sourcePosition(node)}>{alt || "Image"}</span>;
    },
    p({ children, node }) { return <p className="cc-rich-paragraph" {...sourcePosition(node)}>{children}</p>; },
    pre({ children, node }) { return <pre className="cc-rich-code-body" {...sourcePosition(node)}>{children}</pre>; },
    code({ children, className: codeClass, node }) { return <code className={codeClass} {...sourcePosition(node)}>{children}</code>; },
    table({ children, node }) { return <div className="cc-rich-table-wrap"><table className="cc-rich-table" {...sourcePosition(node)}>{children}</table></div>; },
    blockquote({ children, node }) { return <blockquote {...sourcePosition(node)}>{children}</blockquote>; },
    li({ children, node, className: listClass }) { return <li className={listClass} {...sourcePosition(node)}>{children}</li>; },
  }), [urlPolicy]);
  let content: ReactNode;
  if (isJsonDocument(block.source)) {
    content = <pre className="cc-rich-code-body" data-source-start={0} data-source-end={block.source.length}><code className="language-json">{block.source}</code></pre>;
  } else {
    content = <ReactMarkdown remarkPlugins={plugins} remarkRehypeOptions={{ clobberPrefix: `markdown-${encodeURIComponent(block.id)}-` }} components={components} urlTransform={(url) => url}>{block.source}</ReactMarkdown>;
  }
  return (
    <div
      className={["cc-markdown-document", className].filter(Boolean).join(" ")}
      data-markdown-document-id={block.id}
      data-streaming={block.streaming ? "true" : "false"}
    >
      {content}
    </div>
  );
}, (previous, next) => (
  previous.block.id === next.block.id
  && previous.block.source === next.block.source
  && previous.block.streaming === next.block.streaming
  && previous.urlPolicy === next.urlPolicy
  && previous.className === next.className
));
