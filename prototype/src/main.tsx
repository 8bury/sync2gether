import { StrictMode, useState, type ComponentType } from 'react'
import { createRoot } from 'react-dom/client'
import Host from '../examples/s2g-host'
import Guest from '../examples/s2g-guest'
import Prep from '../examples/s2g-prep'
import Mismatch from '../examples/s2g-mismatch'
import Player from '../examples/s2g-player'
import './page.css'

const scenes: { id: string; title: string; note: string; Scene: ComponentType }[] = [
  { id: 'host', title: 'Criar uma sala', note: 'Início com as duas entradas, código em destaque, a outra pessoa entrando.', Scene: Host },
  { id: 'guest', title: 'Entrar com código', note: 'Código digitado no início, erro na linha do campo, progresso no próprio botão.', Scene: Guest },
  { id: 'prep', title: 'Escolher o filme', note: 'As duas cópias lado a lado, a comparação na emenda, o botão diz o que falta.', Scene: Prep },
  { id: 'mismatch', title: 'Arquivos diferentes', note: 'Só o dado que difere em destaque, uma frase com o que fazer.', Scene: Mismatch },
  { id: 'player', title: 'Assistir', note: 'O estado da sala num único selo; avisos curtos de quem pausou e de quem voltou.', Scene: Player },
]

function Scene({ title, note, Scene }: (typeof scenes)[number]) {
  const [run, setRun] = useState(0)
  return (
    <section className="pg-scene">
      <header>
        <div>
          <h2>{title}</h2>
          <p>{note}</p>
        </div>
        <button type="button" onClick={() => setRun((n) => n + 1)}>Repetir</button>
      </header>
      <Scene key={run} />
    </section>
  )
}

// ?capture=<id> shows one scene alone, for filming
const capture = new URLSearchParams(location.search).get('capture')
const shown = capture ? scenes.filter((s) => s.id === capture) : scenes

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <main className="pg">
      {!capture && (
        <div className="pg-intro">
          <h1>Protótipo sync2gether</h1>
          <p>Telas fictícias, sem rede nem mídia real. Referência visual para a interface egui em <code>src/ui</code>.</p>
        </div>
      )}
      {shown.map((s) => <Scene key={s.id} {...s} />)}
    </main>
  </StrictMode>,
)
