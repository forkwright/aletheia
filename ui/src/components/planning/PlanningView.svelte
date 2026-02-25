<script lang="ts">
  import PlanningDashboard from "./PlanningDashboard.svelte";
</script>

<div class="planning-view">
  <PlanningDashboard />
</div>

<style>
  .planning-view {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
    background: var(--bg);
  }
</style>