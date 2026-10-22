import type { Project } from './types';

export interface Snippet {
  title: string;
  description: string;
  code: string;
}

/** Integration snippets shown on the project page. */
export function integrationSnippets(p: Project, origin: string): Snippet[] {
  const key = p.ingest_key;
  return [
    {
      title: 'Hosted page',
      description: 'Link players or users here. No code needed.',
      code: `${origin}/s/${p.slug}`,
    },
    {
      title: 'Floating widget',
      description: 'Adds a feedback button to any web page.',
      code: `<script src="${origin}/widget.js" data-fh-key="${key}" defer></script>`,
    },
    {
      title: 'Inline form',
      description: 'Renders the form inside an element you control.',
      code: `<div data-fh-inline data-fh-key="${key}"></div>\n<script src="${origin}/widget.js" defer></script>`,
    },
    {
      title: 'curl / any HTTP client',
      description: 'Fetch a challenge, solve the proof of work, submit. Games and services do the same in their own language.',
      code: [
        `BASE=${origin}`,
        `KEY=${key}`,
        `CH=$(curl -s "$BASE/api/v1/challenge?key=$KEY")`,
        `TOKEN=$(echo "$CH" | jq -r .challenge)`,
        `BITS=$(echo "$CH" | jq -r .bits)`,
        `NONCE=$(feedback-harvest solve "$TOKEN" "$BITS")`,
        `curl -s -X POST "$BASE/api/v1/feedback" -H 'Content-Type: application/json' \\`,
        `  -d "{\\"key\\":\\"$KEY\\",\\"kind\\":\\"bug\\",\\"title\\":\\"Test\\",\\"body\\":\\"Hello from curl\\",\\"challenge\\":\\"$TOKEN\\",\\"nonce\\":\\"$NONCE\\"}"`,
      ].join('\n'),
    },
  ];
}

export const TOKEN_PLACEHOLDER = '<TOKEN>';

/** MCP client setup snippets for the `/mcp` endpoint. */
export function mcpSnippets(origin: string, token: string = TOKEN_PLACEHOLDER): Snippet[] {
  const url = `${origin}/mcp`;
  const config = {
    mcpServers: {
      'feedback-harvest': {
        type: 'http',
        url,
        headers: { Authorization: `Bearer ${token}` },
      },
    },
  };
  return [
    {
      title: 'Claude Code',
      description: 'Run once in a terminal.',
      code: `claude mcp add --transport http feedback-harvest ${url} --header "Authorization: Bearer ${token}"`,
    },
    {
      title: 'Generic MCP config (JSON)',
      description: 'For clients that read an mcpServers config file.',
      code: JSON.stringify(config, null, 2),
    },
  ];
}
