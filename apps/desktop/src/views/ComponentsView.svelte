<script lang="ts">
  import { Check, LoaderCircle, TriangleAlert, KeyRound, ShieldAlert, Languages } from '$lib/icons';
  import Button from '$lib/components/ui/button.svelte';
  const states = [
    { title: '空内容', body: '等待输入或框选屏幕。', icon: Languages },
    { title: '处理中', body: '正在识别和翻译，支持取消。', icon: LoaderCircle },
    { title: '成功', body: '结果可阅读、复制和校对。', icon: Check },
    { title: '错误', body: '保留原文，提供重试入口。', icon: TriangleAlert },
    { title: '缺少配置', body: '前往服务页填写所需参数。', icon: KeyRound },
    { title: '权限被拒绝', body: '提供系统设置与重新检测入口。', icon: ShieldAlert },
  ];
</script>
<div class="component-gallery"><h2>组件与状态</h2><p class="muted">开发模式专用，使用同一套主题与设计变量。</p><div class="gallery-buttons"><Button>主操作</Button><Button variant="outline">次要操作</Button><Button variant="ghost">轻量操作</Button><Button disabled>不可用</Button></div><div class="state-grid">{#each states as state}<div class="state-card"><state.icon size={23}/><strong>{state.title}</strong><p>{state.body}</p></div>{/each}</div><input class="field" placeholder="输入框状态预览"/><div class="notice green-notice"><Check size={17}/><span>成功与提示使用文字、图标和颜色共同表达。</span></div></div>
