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
  <q-layout view="hHh lpR fFf">
    <q-page-container>
      <q-page class="fullscreen bg-grey-7 flex flex-center">
        <q-card square class="my-card shadow-24 bg-white text-white">
          <q-card-section class="bg-primary">
            <div class="text-h5 q-my-md">ZincObserve</div>
          </q-card-section>
          <q-card>
            <q-card-section class="bg-white">
              <q-form ref="loginform" class="q-gutter-md" @submit.prevent="">
                <q-input v-model="name" data-cy="login-user-id" label="Email *">
                  <template #prepend>
                    <q-icon name="email" />
                  </template>
                </q-input>

                <q-input
                  v-model="password"
                  data-cy="login-password"
                  type="password"
                  :label="t('login.password') + ' *'"
                >
                  <template #prepend>
                    <q-icon name="lock" />
                  </template>
                </q-input>

                <q-card-actions class="q-px-lg q-mt-md q-mb-xl">
                  <q-btn
                    data-cy="login-sign-in"
                    unelevated
                    size="lg"
                    class="full-width"
                    color="primary"
                    type="submit"
                    :label="t('login.signIn')"
                    :loading="submitting"
                    no-caps
                    @click="onSignIn()"
                  />
                </q-card-actions>
              </q-form>
            </q-card-section>
          </q-card>
        </q-card>
      </q-page>
    </q-page-container>
  </q-layout>
</template>

<script lang="ts">
import { defineComponent, ref } from "vue";
import { useStore } from "vuex";
import { useQuasar } from "quasar";
import { useRouter } from "vue-router";

import { useI18n } from "vue-i18n";
import authService from "../../services/auth";
import organizationsService from "../../services/organizations";
import {
  useLocalToken,
  getBasicAuth,
  b64EncodeUnicode,
  useLocalUserInfo,
  useLocalCurrentUser,
  useLocalOrganization,
} from "../../utils/zincutils";
import { getDefaultOrganization, redirectUser } from "../../utils/common";

export default defineComponent({
  name: "PageLogin",

  setup() {
    const store = useStore();
    const router = useRouter();
    const $q = useQuasar();
    const { t } = useI18n();
    const name = ref("");
    const password = ref("");
    const confirmpassword = ref("");
    const email = ref("");
    const loginform = ref();

    const submitting = ref(false);

    const onSignIn = () => {
      if (name.value == "" || password.value == "") {
        $q.notify({
          position: "top",
          color: "warning",
          textColor: "white",
          icon: "warning",
          message: "Please input",
        });
      } else {
        submitting.value = true;
        try {
          authService
            .sign_in_user({
              name: name.value,
              password: password.value,
            })
            .then(async (res: any) => {
              if (res.data.status == true) {
                const authToken = getBasicAuth(name.value, password.value);
                useLocalToken(authToken);
                const userInfo = {
                  given_name: name.value,
                  auth_time: Math.floor(Date.now() / 1000),
                  name: name.value,
                  exp: Math.floor(
                    (new Date().getTime() + 1000 * 60 * 60 * 24 * 30) / 1000
                  ),
                  family_name: "",
                  email: name.value,
                  role: res.data.role,
                };
                const encodedUserInfo = b64EncodeUnicode(
                  JSON.stringify(userInfo)
                );

                useLocalUserInfo(encodedUserInfo);
                store.dispatch("setUserInfo", encodedUserInfo);

                useLocalCurrentUser(JSON.stringify(userInfo));
                store.dispatch("setCurrentUser", userInfo);

                const redirectURI =
                  window.sessionStorage.getItem("redirectURI");
                window.sessionStorage.removeItem("redirectURI");

                const selectedOrg = ref("");
                const orgOptions = ref([{ label: Number, value: String }]);
                await organizationsService
                  .os_list(0, 1000, "id", false, "", "default")
                  .then((res: any) => {
                    const localOrg: any = useLocalOrganization();
                    if (
                      localOrg.value != null &&
                      localOrg.value.user_email !== userInfo.email
                    ) {
                      localOrg.value = null;
                      useLocalOrganization("");
                    }

                    orgOptions.value = res.data.data.map(
                      (data: {
                        id: any;
                        name: any;
                        type: any;
                        identifier: any;
                        user_obj: any;
                        ingest_threshold: number;
                        search_threshold: number;
                        user_email: string;
                      }) => {
                        const optiondata: any = {
                          label: data.name,
                          id: data.id,
                          type: data.type,
                          identifier: data.identifier,
                          user_email: data.user_email,
                          ingest_threshold: data.ingest_threshold,
                          search_threshold: data.search_threshold,
                        };

                        if (
                          ((selectedOrg.value == "" ||
                            selectedOrg.value == undefined) &&
                            data.type == "default" &&
                            userInfo.email == data.user_email) ||
                          res.data.data.length == 1
                        ) {
                          selectedOrg.value = localOrg.value
                            ? localOrg.value
                            : optiondata;
                          useLocalOrganization(selectedOrg.value);

                          store.dispatch(
                            "setSelectedOrganization",
                            selectedOrg.value
                          );
                          redirectUser(redirectURI);
                        }
                        return optiondata;
                      }
                    );

                    return res.data.data;
                  });
              } else {
                submitting.value = false;
                loginform.value.resetValidation();
                $q.notify({
                  color: "negative",
                  message: res.data.message,
                });
              }
            })
            .catch((e: Error) => {
              submitting.value = false;
              loginform.value.resetValidation();
              $q.notify({
                color: "negative",
                message: "Invalid username or password",
              });
              console.log(e);
            });
        } catch (e) {
          submitting.value = false;
          loginform.value.resetValidation();
          $q.notify({
            color: "negative",
            message: "Please fill all the fields and try again.",
          });
          console.log(e);
        }
      }
    };

    return {
      t,
      name,
      password,
      confirmpassword,
      email,
      loginform,
      submitting,
      onSignIn,
      tab: ref("signin"),
      innerTab: ref("signup"),
    };
  },
  methods: {
    selected(item: any) {
      this.$q.notify(`Selected suggestion "${item.label}"`);
    },
  },
});
</script>

<style lang="scss">
.my-card {
  width: 400px;
}
</style>
