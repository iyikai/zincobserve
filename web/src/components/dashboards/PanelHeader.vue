<!-- Copyright 2022 Zinc Labs Inc. and Contributors

 Licensed under the Apache License, Version 2.0 (the "License");
 you may not use this file except in compliance with the License.
 You may obtain a copy of the License at

     http:www.apache.org/licenses/LICENSE-2.0

 Unless required by applicable law or agreed to in writing, software
 distributed under the License is distributed on an "AS IS" BASIS,
 WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 See the License for the specific language governing permissions and
 limitations under the License. 
-->

<template>
  <div>
    <q-bar class="bg-white">
    <q-icon v-if="draggable" name="drag_indicator" />
      <div class="">
        {{ renderTitle }}
    </div>
      <q-space />
      <q-btn-dropdown dense flat label="" no-caps >
        <q-list dense>
          <q-item clickable v-close-popup @click="onPanelModifyClick('EditPanel')">
            <q-item-section>
              <q-item-label class="q-pa-sm">Edit Panel</q-item-label>
            </q-item-section>
          </q-item>
          <q-item clickable v-close-popup @click="onPanelModifyClick('DeletePanel')">
            <q-item-section>
              <q-item-label class="q-pa-sm">Delete Panel</q-item-label>
            </q-item-section>
          </q-item>
        </q-list>
      </q-btn-dropdown>
    </q-bar>
  </div>
</template>

<script lang="ts">
import { computed, defineComponent } from "vue";
import { toRaw } from 'vue';
import { useRouter } from "vue-router";

export default defineComponent({
  name: "PanelHeader",
  props: ["panelDataElement", "dashboardId", "draggable"],
  panelData: [],

  setup(props, { emit }) {
    const panelDataElementObject = toRaw(props.panelDataElement)
    const dashboardId = props.dashboardId
    const router = useRouter();

    const renderTitle = computed(() => {
      return props.panelDataElement.config?.title
    })
    //for edit panel
    const addNewPanel = () => {
      return router.push({
        path: "/addPanel",
        query: { dashboard: String(dashboardId), panelId:panelDataElementObject.id },
      });
    }

    // for delete panel
    const deletePanel = () => {
        return router.push({
        path: "/viewDashboard",
        query: { dashboard: String(dashboardId), panelId:panelDataElementObject.id },
      });
    }

    return {
      panelDataElementObject,
      dashboardId,
      addNewPanel,
      deletePanel,
      renderTitle
    };
  }, methods: {

    onPanelModifyClick(evt: any) {
      if(evt == 'EditPanel'){this.addNewPanel()}
      else if(evt == 'DeletePanel') {
        this.$emit('clicked', this.panelDataElementObject)
      }
      else {
        // console.log(evt)
      }
      
    },
  },
});
</script>