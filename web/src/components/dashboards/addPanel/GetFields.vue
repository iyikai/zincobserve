<template>
    <div class="col" style="height: 100%">
        <div class="q-pa-sm">
            <span class="text-weight-bold">{{ t('panel.fields') }}</span>
        </div>
        <div
            class="scroll"
            style="width: 100%;"
            v-if="true"
        >
            <index-list/>
        </div>
    </div>
</template>

<script lang="ts">
import { defineComponent, onMounted, ref } from "vue"
import { useStore } from "vuex";
import { useI18n } from "vue-i18n";
import { useRouter } from "vue-router";
import IndexList from "./IndexList.vue"
import useDashboardPanelData from "../../../composables/useDashboardPanel";

    export default defineComponent({
        name: "GetFields",
        props:[],
        components: {
            IndexList,
        },
        setup(){
            const store = useStore();
            const router = useRouter();
            const { dashboardPanelData } = useDashboardPanelData()
            const { t } = useI18n();

            return {
                store,
                router,
                dashboardPanelData,
                t              
            }
        }
          
        
    })
</script>

<style lang="sass" scoped>

</style>