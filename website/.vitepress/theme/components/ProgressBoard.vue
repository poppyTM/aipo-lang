<script setup>
import { computed, ref } from 'vue'
import snapshot from '../../../public/progress/tasks.json'
import { filterTasks, percentage, statusLabel, totals, validateProgress } from '../../../progress/core.mjs'

const data = validateProgress(snapshot)
const query = ref('')
const phase = ref('')
const status = ref('')
const summary = computed(() => totals(data.tasks))
const visible = computed(() => filterTasks(data.tasks, { query: query.value, phase: phase.value, status: status.value }))
const groups = computed(() => data.phases.map((item) => ({ ...item, tasks: visible.value.filter((task) => task.phase === item.id) })).filter((item) => item.tasks.length))
const completed = (task) => task.checkpoints.filter((point) => point.completed).length
const date = (value) => value.split('-').reverse().join('/')
const sourceUrl = (path) => `https://github.com/poppyTM/aipo-lang/${/\.[a-z0-9]+$/i.test(path) ? 'blob' : 'tree'}/main/${path.split('/').map(encodeURIComponent).join('/')}`
function clear() { query.value = ''; phase.value = ''; status.value = '' }
</script>

<template>
  <div class="aipo-progress" aria-label="Progresso da implementação do Aipo">
    <header class="ap-head">
      <p class="ap-kicker">Caderno vivo · <code>{{ data.branch }}</code> · snapshot {{ data.baselineSha.slice(0, 8) }}</p>
      <p class="ap-intro">Da especificação à verificação. Cada processo aponta para código, documentação, critérios de aceite e gates ainda pendentes.</p>
      <p class="ap-meta">Atualizado em {{ date(data.updated) }} · {{ data.tasks.length }} processos · <a href="/engineering/progress-protocol">Como atualizar este painel</a></p>
    </header>

    <dl class="ap-totals" aria-label="Contagem dos processos por estado">
      <div><dt>TODO</dt><dd>{{ summary.TODO }}</dd></div>
      <div><dt>IN PROGRESS</dt><dd>{{ summary['IN PROGRESS'] }}</dd></div>
      <div><dt>DONE</dt><dd>{{ summary.DONE }}</dd></div>
    </dl>

    <form class="ap-filters" role="search" aria-label="Buscar e filtrar processos" @submit.prevent>
      <div>
        <label for="ap-query">Buscar processo</label>
        <input id="ap-query" v-model="query" type="search" placeholder="ID, nome, requisito ou lacuna" autocomplete="off">
      </div>
      <div>
        <label for="ap-status">Estado</label>
        <select id="ap-status" v-model="status">
          <option value="">Todos os estados</option>
          <option value="TODO">TODO</option>
          <option value="IN PROGRESS">IN PROGRESS</option>
          <option value="DONE">DONE</option>
        </select>
      </div>
      <div>
        <label for="ap-phase">Etapa</label>
        <select id="ap-phase" v-model="phase">
          <option value="">Todas as etapas</option>
          <option v-for="item in data.phases" :key="item.id" :value="item.id">{{ item.title }}</option>
        </select>
      </div>
      <button type="button" class="ap-clear" @click="clear">Limpar filtros</button>
    </form>

    <p class="ap-warning">{{ data.scope }} Itens marcados como implementados ou com documentação histórica não equivalem a teste executado no commit atual.</p>
    <p class="ap-meta" role="status" aria-live="polite" aria-atomic="true">{{ visible.length }} de {{ data.tasks.length }} processos encontrados</p>

    <template v-if="groups.length">
      <section v-for="group in groups" :key="group.id" class="ap-group" :aria-labelledby="'ap-phase-'+group.id">
        <h2 :id="'ap-phase-'+group.id">{{ group.title }} <span class="ap-phase-count">{{ group.tasks.length }}</span></h2>
        <p class="ap-phase-desc">{{ group.description }}</p>
        <div class="ap-scroll" role="region" :aria-label="'Tabela de processos — '+group.title" tabindex="0">
          <table>
            <caption class="visually-hidden">Estado, critérios e referências para {{ group.title }}</caption>
            <thead><tr><th scope="col">ID</th><th scope="col">Processo e checkpoints</th><th scope="col">Estado</th><th scope="col">Documento</th></tr></thead>
            <tbody>
              <tr v-for="task in group.tasks" :id="task.id" :key="task.id">
                <th scope="row" class="ap-id"><a :href="'#'+task.id" :aria-label="'Link permanente para '+task.id">{{ task.id }}</a></th>
                <td class="ap-main">
                  <a class="ap-title" :href="task.document">{{ task.title }}</a>
                  <p>{{ task.description }}</p>
                  <details>
                    <summary>Critérios e evidências · {{ completed(task) }}/{{ task.checkpoints.length }}</summary>
                    <p class="ap-baseline"><strong>Baseline:</strong> {{ task.baseline }}</p>
                    <ol class="ap-checkpoints">
                      <li v-for="(point, i) in task.checkpoints" :key="i">
                        <span :class="['ap-point', { 'ap-point-done': point.completed }]">{{ point.completed ? 'Concluído' : 'Pendente' }}</span>
                        <span>{{ point.title }}</span>
                        <div v-if="point.completed" class="ap-proof">
                          <a :href="data.evidence[point.evidence].document">Evidência</a>
                          · {{ data.evidence[point.evidence].summary }}
                          · revisão {{ data.evidence[point.evidence].revision.slice(0, 8) }}
                        </div>
                      </li>
                    </ol>
                    <p class="ap-baseline"><strong>Próximo:</strong> {{ task.next }}</p>
                    <p class="ap-baseline"><strong>Gates:</strong> <span v-for="(gate, i) in task.gates" :key="i">{{ i ? ' · ' : '' }}{{ gate.name }}: <strong>{{ gate.status }}</strong></span></p>
                    <details class="ap-sources">
                      <summary>Fontes inspecionadas</summary>
                      <ul>
                        <li v-for="path in [...new Set(task.checkpoints.filter(c => c.completed).flatMap(c => data.evidence[c.evidence].sourcePaths))]" :key="path">
                          <a :href="sourceUrl(path)" target="_blank" rel="noopener noreferrer">{{ path }}</a>
                        </li>
                      </ul>
                    </details>
                  </details>
                </td>
                <td class="ap-state-cell">
                  <span :class="['ap-state', 'ap-'+task.status.toLowerCase().replace(' ', '-')]">{{ statusLabel(task) }}</span>
                  <progress v-if="task.status === 'IN PROGRESS'" :value="percentage(task)" max="100" :aria-label="task.id+': '+percentage(task)+'% dos checkpoints concluídos'">{{ percentage(task) }}%</progress>
                  <small>Atualizado {{ date(task.updated) }}</small>
                </td>
                <td class="ap-doc"><a :href="task.document">Abrir documentação</a></td>
              </tr>
            </tbody>
          </table>
        </div>
      </section>
    </template>
    <div v-else class="ap-empty" role="status">Nenhum processo corresponde aos filtros. <button type="button" @click="clear">Limpar filtros</button>.</div>
    <p class="ap-footnote">Fonte versionada: <a href="https://github.com/poppyTM/aipo-lang/blob/main/website/public/progress/tasks.json">website/public/progress/tasks.json</a>. O painel não inventa resultados, nem consulta código ou pipelines automaticamente.</p>
  </div>
</template>
