import type { ReactNode } from 'react'
import { Equal, FileVideo, FolderOpen, Play, RefreshCw, X } from 'lucide-react'
import { Avatar, Button, Check, Meter, Roll, Spinner, States } from '../../kit'
import { AppWindow } from '../_s2g/parts'
import './prep.css'

// The film step, shared by the matching story (s2g-prep) and the mismatch
// still (s2g-mismatch). The two copies sit side by side as equals, the
// comparison between them is one mark on the seam, and the screen's status
// is said once: on the main button itself, which carries the next thing to
// do and only lights up when it can be done.

export type File = { name: string; size: string; progress: number; state: 'none' | 'checking' | 'ok' | 'bad' }
export type Compare = 'waiting' | 'checking' | 'same' | 'different'
export type Main = 'choose' | 'checking' | 'start' | 'starting' | 'fix'

export type PrepProps = { you: File; ana: File; compare: Compare; main: Main; count?: number }

export function Prep({ step = 1, ...props }: PrepProps & { step?: number }) {
  return (
    <AppWindow step={step} room="4821-gato-lua" className="p2g-win">
      <PrepBody {...props} />
    </AppWindow>
  )
}

export function PrepBody({ you, ana, compare, main, count }: PrepProps) {
  return (
    <div className="p2g-body">
      <div className="p2g-title">
        <Roll variant="fade">{TITLE[compare]}</Roll>
      </div>
      <div className="p2g-pair">
        <Copy who="Você" file={you} action={<Button variant="ghost" size="small" icon={<RefreshCw />} className="p2g-swap" data-hidden={you.state === 'none' || main === 'fix' || undefined} data-target="swap"><span>Trocar</span></Button>} target="choose" />
        <Seam compare={compare} />
        <Copy who="Ana" file={ana} />
      </div>
      <Note compare={compare} />
      <MainButton main={main} count={count} />
    </div>
  )
}

const TITLE: Record<Compare, string> = {
  waiting: 'Escolham o mesmo filme',
  checking: 'Conferindo os arquivos',
  same: 'Tudo pronto',
  different: 'Os arquivos não batem',
}

function Copy({ who, file, action, target }: { who: string; file: File; action?: ReactNode; target?: string }) {
  const empty = file.state === 'none'
  return (
    <div className="p2g-copy" data-state={file.state}>
      <div className="p2g-who">
        <Avatar name={who} size={22} />
        <span>{who}</span>
        {action}
      </div>
      <div className="p2g-file">
        <span className="p2g-icon"><FileVideo /></span>
        <div className="p2g-lanes">
          {/* The empty state and the file share one lane, so choosing a
              file changes the words in place instead of growing the card */}
          <States
            align="start"
            value={empty ? 'empty' : 'file'}
            states={{
              empty: (
                <Button variant="primary" className="s2g-cta p2g-choose" icon={<FolderOpen />} data-target={target} data-cursor="pointer">Escolher arquivo</Button>
              ),
              file: (
                <span className="p2g-meta">
                  <span className="p2g-name">{file.name}</span>
                  <span className="p2g-line">
                    <span className="p2g-size" data-bad={file.state === 'bad' || undefined}>{file.size}</span>
                    <Status file={file} />
                  </span>
                </span>
              ),
            }}
          />
        </div>
      </div>
    </div>
  )
}

function Status({ file }: { file: File }) {
  // Progress, done and the mismatch share one lane and crossfade, so the
  // meter never blinks out and the tick never pops in
  return (
    <States
      align="start"
      className="p2g-status-lane"
      value={file.state === 'checking' ? 'checking' : file.state === 'bad' ? 'bad' : 'ok'}
      states={{
        checking: (
          <span className="p2g-status">
            <Meter value={file.progress} className="p2g-meter" />
            <span className="p2g-pct">{Math.round(file.progress * 100)}%</span>
          </span>
        ),
        ok: (
          <span className="p2g-status p2g-status--ok">
            <Check on size={14} />
            Conferido
          </span>
        ),
        bad: <span className="p2g-status p2g-status--bad">Tamanho diferente</span>,
      }}
    />
  )
}

/** One mark on the seam between the copies: what the comparison says. */
function Seam({ compare }: { compare: Compare }) {
  return (
    <span className="p2g-seam" data-compare={compare}>
      <States
        value={compare}
        states={{
          waiting: <span className="p2g-dot" />,
          checking: <Spinner size={16} />,
          same: <Equal size={16} strokeWidth={2.4} />,
          different: <X size={16} strokeWidth={2.4} />,
        }}
      />
    </span>
  )
}

function Note({ compare }: { compare: Compare }) {
  return (
    <div className="p2g-note" data-compare={compare}>
      <States
        value={compare}
        states={{
          waiting: <span>Cada pessoa escolhe a própria cópia. O app compara o conteúdo, não o nome.</span>,
          checking: <span>Conferindo o conteúdo do seu arquivo…</span>,
          same: <span>Os dois arquivos têm o mesmo conteúdo.</span>,
          different: <span>O arquivo da Ana tem 2,1 GB e o seu tem 4,2 GB. Um de vocês precisa trocar.</span>,
        }}
      />
    </div>
  )
}

const LABEL: Record<Main, string> = {
  choose: 'Escolha seu arquivo para começar',
  checking: 'Conferindo seu arquivo…',
  start: 'Começar a assistir',
  starting: 'Começando…',
  fix: 'Trocar meu arquivo',
}

function MainButton({ main, count }: { main: Main; count?: number }) {
  const live = main === 'start' || main === 'starting'
  const fix = main === 'fix'
  return (
    <Button
      variant={live ? 'primary' : 'secondary'}
      className="s2g-cta p2g-main"
      data-off={(!live && !fix) || undefined}
      data-target="start"
      data-cursor={live || fix ? 'pointer' : undefined}
      icon={fix ? <RefreshCw /> : <Play />}
    >
      <Roll variant="fade">{main === 'starting' ? `Começando em ${count ?? 3}…` : LABEL[main]}</Roll>
    </Button>
  )
}
