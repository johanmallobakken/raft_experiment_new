; ModuleID = 'probe2.a7a4e74a-cgu.0'
source_filename = "probe2.a7a4e74a-cgu.0"
target datalayout = "e-m:o-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-apple-macosx10.7.0"

; probe2::probe
; Function Attrs: uwtable
define void @_ZN6probe25probe17h4f82b48bade8eb17E() unnamed_addr #0 {
start:
; call core::f64::<impl f64>::to_int_unchecked
  %_1 = call i32 @"_ZN4core3f6421_$LT$impl$u20$f64$GT$16to_int_unchecked17h519d6649e78af81aE"(double 1.000000e+00)
  br label %bb1

bb1:                                              ; preds = %start
  ret void
}

; core::f64::<impl f64>::to_int_unchecked
; Function Attrs: inlinehint uwtable
define i32 @"_ZN4core3f6421_$LT$impl$u20$f64$GT$16to_int_unchecked17h519d6649e78af81aE"(double %self) unnamed_addr #1 {
start:
; call <f64 as core::convert::num::FloatToInt<i32>>::to_int_unchecked
  %0 = call i32 @"_ZN65_$LT$f64$u20$as$u20$core..convert..num..FloatToInt$LT$i32$GT$$GT$16to_int_unchecked17h7733b29e5948e6d2E"(double %self)
  br label %bb1

bb1:                                              ; preds = %start
  ret i32 %0
}

; <f64 as core::convert::num::FloatToInt<i32>>::to_int_unchecked
; Function Attrs: inlinehint uwtable
define internal i32 @"_ZN65_$LT$f64$u20$as$u20$core..convert..num..FloatToInt$LT$i32$GT$$GT$16to_int_unchecked17h7733b29e5948e6d2E"(double %self) unnamed_addr #1 {
start:
  %0 = alloca i32, align 4
  %1 = fptosi double %self to i32
  store i32 %1, i32* %0, align 4
  %2 = load i32, i32* %0, align 4
  br label %bb1

bb1:                                              ; preds = %start
  ret i32 %2
}

attributes #0 = { uwtable "frame-pointer"="all" "probe-stack"="__rust_probestack" "target-cpu"="core2" }
attributes #1 = { inlinehint uwtable "frame-pointer"="all" "probe-stack"="__rust_probestack" "target-cpu"="core2" }

!llvm.module.flags = !{!0}

!0 = !{i32 7, !"PIC Level", i32 2}
