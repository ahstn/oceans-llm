import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { SkillOwnerActions, VersionMetadata } from './$id'
import { SkillInstructions } from './-files'
import { SkillUploadDialog } from './-upload-dialog'
import type { SkillSummary, SkillVersionDetail } from '@/types/skills-api'

const { saveSkillNamespace, uploadSkill } = vi.hoisted(() => ({
  saveSkillNamespace: vi.fn(),
  uploadSkill: vi.fn(),
}))

vi.mock('@/server/skills-data.functions', () => ({
  saveSkillNamespace,
  uploadSkill,
  getSkill: vi.fn(),
  getSkillFile: vi.fn(),
  saveSkillDefault: vi.fn(),
}))

const skill: SkillSummary = {
  id: '11111111-1111-4111-8111-111111111111',
  namespace: 'alex',
  owner_user_id: 'owner-id',
  name: 'code-review',
  description: 'Review a change.',
  default_version: 1,
  latest_version: 2,
  created_at: '2026-10-03T00:00:00Z',
  updated_at: '2026-10-03T00:00:00Z',
}

const limits = {
  max_archive_bytes: 10 * 1024 * 1024,
  max_expanded_bytes: 25 * 1024 * 1024,
  max_files: 1000,
}

beforeEach(() => vi.resetAllMocks())
afterEach(cleanup)

describe('skill ownership controls', () => {
  it('hides all mutations from a different user', () => {
    render(
      <SkillOwnerActions
        skill={skill}
        userId="another-user"
        version={2}
        pending={false}
        onUpload={vi.fn()}
        onSetDefault={vi.fn()}
      />,
    )
    expect(screen.queryByRole('button')).not.toBeInTheDocument()
  })

  it('allows the owner to select a newer default, but disables the existing default', () => {
    const onSetDefault = vi.fn()
    const props = { skill, userId: 'owner-id', pending: false, onUpload: vi.fn(), onSetDefault }
    const view = render(<SkillOwnerActions {...props} version={2} />)
    fireEvent.click(screen.getByRole('button', { name: 'Set as default' }))
    expect(onSetDefault).toHaveBeenCalledOnce()
    view.rerender(<SkillOwnerActions {...props} version={1} />)
    expect(screen.getByRole('button', { name: 'Set as default' })).toBeDisabled()
    expect(screen.getByRole('button', { name: 'Upload new version' })).toBeEnabled()
  })
})

describe('skill previews', () => {
  it('renders instructions without frontmatter, raw HTML, active URLs, or remote image loads', () => {
    const { container } = render(
      <SkillInstructions
        source={
          '---\nname: code-review\ndescription: Review a change.\n---\n# Review\n\n<script>alert(1)</script>\n\n[unsafe](javascript:alert%281%29)\n\n![tracker](https://example.com/track.png)\n\n[Reference](https://example.com/docs)'
        }
      />,
    )
    expect(screen.getByRole('heading', { name: 'Review' })).toBeVisible()
    expect(screen.queryByText('name: code-review')).not.toBeInTheDocument()
    expect(container.querySelector('script')).toBeNull()
    expect(container.querySelector('img')).toBeNull()
    expect(screen.getAllByRole('link')).toHaveLength(1)
    expect(screen.getByRole('link', { name: 'Reference' })).toHaveAttribute(
      'rel',
      'noopener noreferrer',
    )
  })
})

describe('skill attribution', () => {
  const detail: SkillVersionDetail = {
    version: {
      version: 2,
      sha256: 'a'.repeat(64),
      archive_bytes: 1024,
      extracted_bytes: 2048,
      file_count: 1,
      created_at: '2026-10-03T15:07:59+02:00',
    },
    manifest: {
      name: 'code-review',
      description: 'Review a change.',
    },
    files: [],
    instructions: '# Review',
  }

  function withMetadata(metadata: SkillVersionDetail['manifest']['metadata']): SkillVersionDetail {
    return { ...detail, manifest: { ...detail.manifest, metadata } }
  }

  it('shows attribution for the selected skill version', () => {
    const view = render(
      <VersionMetadata
        detail={withMetadata({
          author: ' Matt Pocock ',
          version: ' 1.0 ',
          github: 'https://github.com/mattpocock/skills/tree/main/grill-me',
        })}
      />,
    )
    expect(screen.getByText('Matt Pocock')).toBeVisible()
    expect(screen.getByText('Upstream version').nextElementSibling).toHaveTextContent('1.0')
    expect(screen.getByText('Uploaded').nextElementSibling).toHaveTextContent(/^03\/10\/26 13:07$/)
    expect(screen.getByText('Contents').nextElementSibling).toHaveTextContent(/^1 file$/)
    expect(screen.queryByText('SHA-256')).not.toBeInTheDocument()
    expect(screen.queryByText(detail.version.sha256)).not.toBeInTheDocument()
    expect(screen.queryByText(/bytes/)).not.toBeInTheDocument()
    const source = screen.getByRole('link', { name: 'View on GitHub' })
    expect(source).toHaveAttribute(
      'href',
      'https://github.com/mattpocock/skills/tree/main/grill-me',
    )
    expect(source).toHaveAttribute('target', '_blank')
    expect(source).toHaveAttribute('rel', 'noopener noreferrer')

    view.rerender(
      <VersionMetadata
        detail={{
          ...withMetadata({ author: 'Earlier author', version: 'release-candidate' }),
          version: { ...detail.version, version: 1, file_count: 2 },
        }}
      />,
    )
    expect(screen.getByText('Earlier author')).toBeVisible()
    expect(screen.getByText('release-candidate')).toBeVisible()
    expect(screen.getByText('Contents').nextElementSibling).toHaveTextContent(/^2 files$/)
    expect(screen.queryByText('Matt Pocock')).not.toBeInTheDocument()
    expect(screen.queryByRole('link')).not.toBeInTheDocument()
  })

  it.each([undefined, {}, { author: ' ', version: '\n', github: '\t' }])(
    'omits missing or blank attribution without affecting the version details',
    (metadata) => {
      render(<VersionMetadata detail={withMetadata(metadata)} />)
      expect(screen.queryByText('Author')).not.toBeInTheDocument()
      expect(screen.queryByText('Upstream version')).not.toBeInTheDocument()
      expect(screen.queryByText('Source')).not.toBeInTheDocument()
      expect(screen.getByText('Uploaded')).toBeVisible()
    },
  )

  it('renders author text without executing markup', () => {
    const author = '<img src=x onerror=alert(1)>'
    const { container } = render(<VersionMetadata detail={withMetadata({ author })} />)
    expect(screen.getByText(author)).toBeVisible()
    expect(container.querySelector('img')).toBeNull()
  })

  it.each([
    'not a URL',
    'javascript:alert(1)',
    'http://github.com/owner/repo',
    '//github.com/owner/repo',
    'https://github.com',
    'https://github.com/owner',
    'https://github.com.evil.example/owner/repo',
    'https://evil.example/github.com/owner/repo',
    'https://user:password@github.com/owner/repo',
    'https://github.com:8443/owner/repo',
    'https://github.com/owner/repo\\path',
    'https://git\nhub.com/owner/repo',
    'https://github.com/owner/%2e%2e',
    'https://github.com/owner/repo%2Fother',
  ])('omits an invalid GitHub source link: %s', (github) => {
    render(<VersionMetadata detail={withMetadata({ github, author: 'Creator' })} />)
    expect(screen.queryByRole('link')).not.toBeInTheDocument()
    expect(screen.queryByText('Source')).not.toBeInTheDocument()
    expect(screen.getByText('Creator')).toBeVisible()
  })
})

describe('skill uploads', () => {
  it('claims a permanent namespace before accepting an archive', async () => {
    saveSkillNamespace.mockResolvedValue({ handle: 'alex', user_id: 'owner-id' })
    render(
      <SkillUploadDialog namespace={null} limits={limits} onClose={vi.fn()} onUploaded={vi.fn()} />,
    )
    expect(screen.queryByLabelText('ZIP archive')).not.toBeInTheDocument()
    fireEvent.change(screen.getByLabelText('Namespace'), { target: { value: 'alex' } })
    fireEvent.click(screen.getByRole('button', { name: 'Claim namespace' }))
    expect(await screen.findByLabelText('ZIP archive')).toBeVisible()
    expect(saveSkillNamespace).toHaveBeenCalledWith({ data: { handle: 'alex' } })
    expect(uploadSkill).not.toHaveBeenCalled()
  })

  it('rejects oversized archives before sending them', async () => {
    const archive = new File(['zip'], 'skill.zip', { type: 'application/zip' })
    Object.defineProperty(archive, 'size', { value: 10 * 1024 * 1024 + 1 })
    render(
      <SkillUploadDialog
        namespace={{ handle: 'alex', user_id: 'owner-id' }}
        limits={limits}
        onClose={vi.fn()}
        onUploaded={vi.fn()}
      />,
    )
    fireEvent.change(screen.getByLabelText('ZIP archive'), { target: { files: [archive] } })
    fireEvent.submit(screen.getByLabelText('ZIP archive').closest('form')!)
    expect(await screen.findByRole('alert')).toHaveTextContent('10 MiB or smaller')
    expect(uploadSkill).not.toHaveBeenCalled()
  })

  it('keeps a failed upload open and shows the server validation error', async () => {
    uploadSkill.mockRejectedValue(new Error('SKILL.md is required.'))
    const onUploaded = vi.fn()
    render(
      <SkillUploadDialog
        namespace={{ handle: 'alex', user_id: 'owner-id' }}
        limits={limits}
        onClose={vi.fn()}
        onUploaded={onUploaded}
      />,
    )
    fireEvent.change(screen.getByLabelText('ZIP archive'), {
      target: { files: [new File(['zip'], 'skill.zip')] },
    })
    fireEvent.submit(screen.getByLabelText('ZIP archive').closest('form')!)
    await waitFor(() =>
      expect(screen.getByRole('alert')).toHaveTextContent('SKILL.md is required.'),
    )
    expect(onUploaded).not.toHaveBeenCalled()
    expect(screen.getByRole('button', { name: 'Upload skill' })).toBeEnabled()
  })

  it('accepts larger uploads when the gateway limit allows them', async () => {
    uploadSkill.mockResolvedValue({ skill, versions: [], uploaded_version: 2 })
    const archive = new File(['zip'], 'skill.zip')
    Object.defineProperty(archive, 'size', { value: 12 * 1024 * 1024 })
    render(
      <SkillUploadDialog
        namespace={{ handle: 'alex', user_id: 'owner-id' }}
        limits={{ ...limits, max_archive_bytes: 15 * 1024 * 1024 }}
        onClose={vi.fn()}
        onUploaded={vi.fn()}
      />,
    )
    expect(screen.getByText(/15 MiB ZIP/)).toBeVisible()
    fireEvent.change(screen.getByLabelText('ZIP archive'), { target: { files: [archive] } })
    fireEvent.submit(screen.getByLabelText('ZIP archive').closest('form')!)
    await waitFor(() => expect(uploadSkill).toHaveBeenCalledOnce())
  })
})
