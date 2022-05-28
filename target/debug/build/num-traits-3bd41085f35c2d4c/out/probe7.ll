; ModuleID = 'probe7.264af6fe-cgu.0'
source_filename = "probe7.264af6fe-cgu.0"
target datalayout = "e-m:o-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-apple-macosx10.7.0"

; probe7::probe
; Function Attrs: uwtable
define void @_ZN6probe75probe17h98e4efd9fe4d4be6E() unnamed_addr #0 {
start:
; call std::f64::<impl f64>::copysign
  %_1 = call double @"_ZN3std3f6421_$LT$impl$u20$f64$GT$8copysign17hca501f99a7f89beaE"(double 1.000000e+00, double -1.000000e+00)
  br label %bb1

bb1:                                              ; preds = %start
  ret void
}

; std::f64::<impl f64>::copysign
; Function Attrs: inlinehint uwtable
define internal double @"_ZN3std3f6421_$LT$impl$u20$f64$GT$8copysign17hca501f99a7f89beaE"(double %self, double %sign) unnamed_addr #1 {
start:
  %0 = alloca double, align 8
  %1 = call double @llvm.copysign.f64(double %self, double %sign)
  store double %1, double* %0, align 8
  %2 = load double, double* %0, align 8
  br label %bb1

bb1:                                              ; preds = %start
  ret double %2
}

; Function Attrs: nofree nosync nounwind readnone speculatable willreturn
declare double @llvm.copysign.f64(double, double) #2

attributes #0 = { uwtable "frame-pointer"="all" "probe-stack"="__rust_probestack" "target-cpu"="core2" }
attributes #1 = { inlinehint uwtable "frame-pointer"="all" "probe-stack"="__rust_probestack" "target-cpu"="core2" }
attributes #2 = { nofree nosync nounwind readnone speculatable willreturn }

!llvm.module.flags = !{!0}

!0 = !{i32 7, !"PIC Level", i32 2}
