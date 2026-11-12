/**
 * The landing page as Markdown, for agents and `Accept: text/markdown`
 * readers. Built from the same content module as src/pages/index.astro, so
 * the two can't drift apart.
 */
import {
  audiences,
  bento,
  bentoSection,
  closing,
  deploySection,
  deployments,
  faq,
  harnessSection,
  harnesses,
  hero,
  lifecycle,
  meta,
  pillars,
  statement,
  type Run,
} from "@/landing/content";

export const prerender = true;

const inline = (runs: Run[]) =>
  runs.map((run) => (run.href ? `[${run.text}](${run.href})` : run.code ? `\`${run.text}\`` : run.text)).join("");
const link = ({ label, href }: { label: string; href: string }) => `[${label}](${href})`;
const list = (items: string[]) => items.map((item) => `- ${item}`).join("\n");

export function GET() {
  const sections = [
    `# ${meta.title}`,
    `**${hero.title} ${hero.subtitle}**`,
    hero.lede,
    `${link(hero.primary)} · ${link(hero.secondary)}`,
    inline(statement),
    ...audiences.map((a) => `## ${a.title}\n\n${list(a.items)}`),
    `## ${lifecycle.title}\n\n${lifecycle.lede}\n\n${lifecycle.steps
      .map((step, i) => `${i + 1}. **${step.title}.** ${step.body}`)
      .join("\n")}\n\nSee ${link(lifecycle.link)}.`,
    ...pillars.map(
      (p) => `## ${p.title}\n\n${inline(p.body)}\n\n${list(p.points)}\n\nSee ${link(p.link)}.`,
    ),
    `## ${harnessSection.title}\n\n${harnessSection.lede}\n\n${harnesses
      .map(
        (h) =>
          `### ${h.label}\n\n${h.note}\n\n${h.files
            .map((f) => `\`${f.path}\`:\n\n\`\`\`${f.lang}\n${f.code}\n\`\`\``)
            .join("\n\n")}`,
      )
      .join("\n\n")}\n\nSee ${link(harnessSection.link)}.`,
    `## ${bentoSection.title}\n\n${bento
      .map((b) => `- **${b.title}.** ${b.body} See ${link(b.link)}.`)
      .join("\n")}`,
    `## ${deploySection.title}\n\n${deploySection.lede}\n\n${deployments
      .map(
        (d) =>
          `### ${d.title} (${d.store})\n\n${d.body}\n\n\`\`\`sh\n${d.command}\n\`\`\`\n\nSee ${link(d.link)}.`,
      )
      .join("\n\n")}`,
    `## Questions\n\n${faq.map((q) => `### ${q.question}\n\n${inline(q.answer)}`).join("\n\n")}`,
    `## ${closing.title}\n\n${closing.lede}\n\n${link(closing.primary)} · ${link(closing.secondary)}`,
  ];

  return new Response(`${sections.join("\n\n")}\n`, {
    headers: { "Content-Type": "text/markdown; charset=utf-8" },
  });
}
