import { ArrowRight, Check as Tick, Plus } from 'lucide-react'
import { Button, Field, Spinner, States } from '../../kit'
import { AppMark } from './parts'
import './home.css'

// The start screen: both ways in on one screen. Creating is the primary
// action; joining is a code field right under it, so a guest never visits a
// screen of its own just to type. The join result is said in the field's
// own lane (form-error), and the Entrar button carries its progress.

export type Join = 'idle' | 'error' | 'connecting' | 'joined'

export function Home({ on, code = '', focused, join = 'idle' }: { on: boolean; code?: string; focused?: boolean; join?: Join }) {
  return (
    <section className="s2g-screen s2g-home" data-on={on || undefined}>
      <AppMark size={56} />
      <div className="s2g-home__title">Assistir junto</div>
      <div className="s2g-home__sub">Cada pessoa abre a sua cópia do filme. Play, pause e posição ficam iguais nos dois PCs.</div>
      <Button variant="primary" className="s2g-cta s2g-home__wide" icon={<Plus />} data-target="create" data-cursor="pointer">Criar sala</Button>
      <div className="s2g-home__or"><span>ou entre com um código</span></div>
      <div className="s2g-home__join">
        <Field placeholder="Ex.: 4821-gato-lua" value={code} focused={focused} status={join === 'error' ? 'error' : undefined} className="s2g-home__field" box={{ 'data-target': 'code', 'data-cursor': 'ibeam' }} />
        <Button variant={code ? 'primary' : 'secondary'} className="s2g-cta s2g-home__enter" data-target="enter" data-cursor="pointer">
          <States
            value={join === 'connecting' ? 'busy' : join === 'joined' ? 'done' : 'idle'}
            states={{
              idle: <span className="s2g-home__label"><ArrowRight size={16} />Entrar</span>,
              busy: <span className="s2g-home__label"><Spinner size={16} />Entrando</span>,
              done: <span className="s2g-home__label"><Tick size={16} />Entrou</span>,
            }}
          />
        </Button>
      </div>
      <div className="s2g-home__lane" data-error={join === 'error' || undefined}>
        <States
          align="start"
          value={join === 'error' ? 'error' : 'none'}
          states={{
            none: <span />,
            error: <span>Nenhuma sala aberta com esse código.</span>,
          }}
        />
      </div>
      <span className="s2g-link s2g-link--accent s2g-home__solo">Abrir filme sozinho</span>
    </section>
  )
}
