import awsIcon from '@lobehub/icons-static-svg/icons/aws.svg'
import cloudflareIcon from '@lobehub/icons-static-svg/icons/cloudflare.svg'
import exaIcon from '@lobehub/icons-static-svg/icons/exa.svg'
import figmaIcon from '@lobehub/icons-static-svg/icons/figma.svg'
import githubIcon from '@lobehub/icons-static-svg/icons/github.svg'
import googleIcon from '@lobehub/icons-static-svg/icons/google.svg'
import huggingFaceIcon from '@lobehub/icons-static-svg/icons/huggingface.svg'
import n8nIcon from '@lobehub/icons-static-svg/icons/n8n.svg'
import notionIcon from '@lobehub/icons-static-svg/icons/notion.svg'
import obsidianIcon from '@lobehub/icons-static-svg/icons/obsidian.svg'
import snowflakeIcon from '@lobehub/icons-static-svg/icons/snowflake.svg'
import { McpServerIcon } from '@hugeicons/core-free-icons'
import { AppIcon } from '@/components/icons/app-icon'

type McpIconSubject = {
  catalog_key?: string
  display_name: string
  server_key?: string
  server_url?: string
}

const MCP_LOBE_ICON_MATCHERS = [
  { aliases: ['github'], src: githubIcon },
  { aliases: ['snowflake'], src: snowflakeIcon },
  { aliases: ['notion'], src: notionIcon },
  { aliases: ['google'], src: googleIcon },
  { aliases: ['figma'], src: figmaIcon },
  { aliases: ['aws', 'amazon web services'], src: awsIcon },
  { aliases: ['cloudflare'], src: cloudflareIcon },
  { aliases: ['exa'], src: exaIcon },
  { aliases: ['huggingface', 'hugging face'], src: huggingFaceIcon },
  { aliases: ['n8n'], src: n8nIcon },
  { aliases: ['obsidian'], src: obsidianIcon },
] as const

export function McpServerIconMark({
  server,
  size = 18,
  bare = false,
}: {
  server: McpIconSubject
  size?: number
  bare?: boolean
}) {
  const iconSrc = resolveMcpLobeIcon(server)

  const icon = iconSrc ? (
    <img
      alt=""
      aria-hidden="true"
      className="shrink-0 object-contain"
      src={iconSrc}
      style={{
        filter: bare ? 'brightness(0) invert(1)' : 'brightness(0) invert(0.72)',
        height: size,
        width: size,
      }}
    />
  ) : (
    <AppIcon icon={McpServerIcon} size={size} stroke={1.5} aria-hidden />
  )

  if (bare) {
    return icon
  }

  return (
    <span className="bg-muted text-muted-foreground flex size-8 shrink-0 items-center justify-center rounded-md">
      {icon}
    </span>
  )
}

function resolveMcpLobeIcon(server: McpIconSubject) {
  const searchableText = [
    server.server_key,
    server.catalog_key,
    server.display_name,
    server.server_url,
  ]
    .filter(Boolean)
    .join(' ')
    .toLowerCase()

  const normalizedText = searchableText.replace(/[^a-z0-9]+/g, ' ')

  return MCP_LOBE_ICON_MATCHERS.find(({ aliases }) =>
    aliases.some((alias) => {
      const normalizedAlias = alias
        .toLowerCase()
        .replace(/[^a-z0-9]+/g, ' ')
        .trim()
      if (!normalizedAlias) {
        return false
      }
      const escapedAlias = normalizedAlias
        .replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
        .replace(/\s+/g, '\\s+')
      return new RegExp(`(^|\\s)${escapedAlias}(\\s|$)`).test(normalizedText)
    }),
  )?.src
}
